#[cfg(feature = "websocket")]
pub mod websocket;

use std::sync::RwLock;

use bevy::{
    ecs::system::SystemId,
    prelude::*,
    remote::{BrpError, BrpRequest, BrpResponse, BrpResult, error_codes},
    utils::HashMap,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use smol::channel::{self, Receiver, Sender};

const CHANNEL_SIZE: usize = 16;

/// Plugin for managing remote method streams.
///
/// This plugin allows defining methods that can be streamed from a Bevy application
/// to a remote client. It handles the registration of stream handlers and manages
/// active client connections.
pub struct RemoteStreamPlugin {
    methods: RwLock<Vec<(String, RemoteStreamHandlersBuilder)>>,
}

impl RemoteStreamPlugin {
    /// Add a streaming remote method to the plugin using the given `name` and `handler`.
    /// The handler will be called every frame when there is a client connected to the stream.
    /// The handler should return a `None` to indicate that there is nothing to stream.
    /// And return `Some(BrpErr)` to stop the stream.
    #[must_use]
    pub fn with_method(
        mut self,
        name: impl Into<String>,
        handlers: RemoteStreamHandlersBuilder,
    ) -> Self {
        self.methods
            .get_mut()
            .unwrap()
            .push((name.into(), handlers));
        self
    }
}

impl Default for RemoteStreamPlugin {
    fn default() -> Self {
        Self {
            methods: RwLock::new(vec![]), // Initialize with an empty list of methods.
        }
    }
}

impl Plugin for RemoteStreamPlugin {
    /// Builds the plugin by registering systems and resources.
    ///
    /// This function initializes `StreamMethods` and `ActiveStreams` resources,
    /// sets up communication channels, and registers systems for processing
    /// remote requests and handling application exit.
    fn build(&self, app: &mut App) {
        let mut stream_methods = StreamMethods::default();

        let plugin_methods = &mut *self.methods.write().unwrap();

        // Register all defined stream methods.
        for (name, systems) in plugin_methods.drain(..) {
            stream_methods.insert(
                name,
                RemoteStreamHandlers {
                    on_connect: systems
                        .on_connect
                        .map(|sys| app.main_mut().world_mut().register_boxed_system(sys)),
                    on_disconnect: systems
                        .on_disconnect
                        .map(|sys| app.main_mut().world_mut().register_boxed_system(sys)),
                    update: app
                        .main_mut()
                        .world_mut()
                        .register_boxed_system(systems.update),
                    on_data: systems
                        .on_data
                        .map(|sys| app.main_mut().world_mut().register_boxed_system(sys)),
                },
            );
        }

        app.insert_resource(stream_methods)
            .init_resource::<ActiveStreams>()
            .add_systems(PreStartup, setup_channel) // Setup communication channels.
            .add_systems(Update, process_remote_requests) // Process incoming remote requests.
            .add_systems(Update, on_app_exit.run_if(on_event::<AppExit>)); // Handle application exit.
    }
}

/// Defines the handlers for a remote stream.
///
/// This struct holds `SystemId`s for various events in a stream's lifecycle:
/// connection, disconnection, data reception, and per-frame updates.
#[derive(Debug, Clone)]
pub struct RemoteStreamHandlers {
    /// System to run when a client connects to the stream.
    pub on_connect: Option<StreamHandler>,
    /// System to run when a client disconnects from the stream.
    pub on_disconnect: Option<SystemId<StreamHandlerInputRef<'static>>>,
    /// System to run when data is received from the client.
    pub on_data: Option<OnDataHandler>,
    /// System to run every frame for an active stream.
    pub update: StreamHandler,
}

/// Input parameters for stream handlers.
pub struct StreamHandlerInput {
    /// Unique identifier for the client connected to the stream.
    pub client_id: StreamClientId,
    /// Optional parameters provided by the client during connection or data sending.
    pub params: Option<Value>,
}

/// A Bevy `InRef` type for `StreamHandlerInput`.
pub type StreamHandlerInputRef<'a> = InRef<'a, StreamHandlerInput>;
/// A Bevy `SystemId` for a stream handler system.
/// The system takes `StreamHandlerInputRef` and returns an optional `BrpResult`.
pub type StreamHandler = SystemId<StreamHandlerInputRef<'static>, Option<BrpResult>>;
/// A Bevy `In` type for `OnDataHandler` systems, containing client ID and request.
pub type OnDataHandlerInput = In<(StreamClientId, BrpRequest)>;
/// A Bevy `SystemId` for an `on_data` handler system.
/// The system takes `OnDataHandlerInput` and returns an optional `BrpResult`.
pub type OnDataHandler = SystemId<OnDataHandlerInput, Option<BrpResult>>;

/// Builder for creating `RemoteStreamHandlers`.
///
/// This struct allows for a fluent interface to define the systems
/// that will handle different stream events.
#[derive(Debug)]
pub struct RemoteStreamHandlersBuilder {
    /// System to run when a client connects.
    on_connect:
        Option<Box<dyn System<In = StreamHandlerInputRef<'static>, Out = Option<BrpResult>>>>,
    /// System to run when a client disconnects.
    on_disconnect: Option<Box<dyn System<In = StreamHandlerInputRef<'static>, Out = ()>>>,
    /// System to run when data is received from the client.
    on_data: Option<Box<dyn System<In = OnDataHandlerInput, Out = Option<BrpResult>>>>,
    /// System to run every frame for an active stream.
    update: Box<dyn System<In = StreamHandlerInputRef<'static>, Out = Option<BrpResult>>>,
}

impl RemoteStreamHandlersBuilder {
    /// Creates a new `RemoteStreamHandlersBuilder` with the given update handler.
    ///
    /// The `update` handler is mandatory and will be called every frame for active streams.
    pub fn new<M>(
        update: impl IntoSystem<StreamHandlerInputRef<'static>, Option<BrpResult>, M>,
    ) -> Self {
        Self {
            on_connect: None,
            on_disconnect: None,
            on_data: None,
            update: Box::new(IntoSystem::into_system(update)),
        }
    }

    /// Sets the `on_connect` handler for the stream.
    ///
    /// This handler is called when a client initiates a connection to the stream.
    pub fn on_connect<M>(
        mut self,
        system: impl IntoSystem<StreamHandlerInputRef<'static>, Option<BrpResult>, M>,
    ) -> Self {
        self.on_connect = Some(Box::new(IntoSystem::into_system(system)));
        self
    }

    /// Sets the `on_disconnect` handler for the stream.
    ///
    /// This handler is called when a client disconnects or the stream is terminated.
    pub fn on_disconnect<M>(
        mut self,
        system: impl IntoSystem<StreamHandlerInputRef<'static>, (), M>,
    ) -> Self {
        self.on_disconnect = Some(Box::new(IntoSystem::into_system(system)));
        self
    }

    /// Sets the `on_data` handler for the stream.
    ///
    /// This handler is called when the client sends data over an active stream.
    pub fn on_data<M>(
        mut self,
        system: impl IntoSystem<OnDataHandlerInput, Option<BrpResult>, M>,
    ) -> Self {
        self.on_data = Some(Box::new(IntoSystem::into_system(system)));
        self
    }
}

/// Holds all implementations of methods known to the server.
#[derive(Debug, Resource, Default)]
pub struct StreamMethods(HashMap<String, RemoteStreamHandlers>);

impl StreamMethods {
    /// Adds a new method, replacing any existing method with that name.
    ///
    /// If there was an existing method with that name, returns its handler.
    pub fn insert(
        &mut self,
        method_name: impl Into<String>,
        handler: RemoteStreamHandlers,
    ) -> Option<RemoteStreamHandlers> {
        self.0.insert(method_name.into(), handler)
    }
}

/// Sender part of the MPSC channel for stream messages.
/// Used by external systems (e.g., WebSocket server) to send messages to the Bevy world.
#[derive(Resource, Deref, DerefMut)]
pub struct StreamSender(Sender<StreamMessage>);

/// Receiver part of the MPSC channel for stream messages.
/// Used by `process_remote_requests` system to receive messages within the Bevy world.
#[derive(Resource, Deref, DerefMut)]
pub struct StreamReceiver(Receiver<StreamMessage>);

/// Represents a message related to a stream, to be processed by the Bevy application.
pub struct StreamMessage {
    /// The client ID associated with this message.
    client_id: StreamClientId,
    /// The kind of stream message.
    kind: StreamMessageKind,
}

/// A message specifically for establishing a BRP (Bevy Remote Protocol) stream.
/// This is typically sent when a client wants to initiate a named stream.
#[derive(Clone)]
pub struct BrpStreamMessage {
    /// The request method.
    pub method: String,

    /// The request params.
    pub params: Option<Value>,

    /// The channel on which the response is to be sent.
    ///
    /// The value sent here is serialized and sent back to the client.
    pub sender: Sender<BrpResponse>,
}

/// Enumerates the different kinds of messages that can be sent over a stream.
pub enum StreamMessageKind {
    /// Indicates a client is attempting to connect to a stream.
    /// Contains the optional request ID, and the BRP stream message details.
    Connect(Option<Value>, BrpStreamMessage),
    /// Indicates a client has disconnected.
    Disconnect,
    /// Indicates data received from an already connected client.
    Data(Value),
}

/// Resource holding all currently active streams, keyed by `StreamClientId`.
#[derive(Resource, Deref, DerefMut, Default)]
struct ActiveStreams(HashMap<StreamClientId, ActiveStream>);

/// Represents an active stream connection with a client.
struct ActiveStream {
    /// The original request ID from the client, if any, for the stream connection.
    request_id: Option<Value>,
    /// Sender to send responses back to the client for this specific stream.
    sender: ActiveStreamSender,
    /// Input parameters for this stream, including client ID and connection params.
    input: StreamHandlerInput,
    /// System ID for the per-frame update handler for this stream.
    on_update: StreamHandler,
    /// System ID for the disconnect handler for this stream.
    on_disconnect: Option<SystemId<StreamHandlerInputRef<'static>>>,
    /// System ID for the data handler for this stream.
    on_data: Option<OnDataHandler>,
}

/// Wrapper around a `Sender<BrpResponse>` for an active stream.
/// Provides a convenient `send` method that formats the `BrpResponse`.
struct ActiveStreamSender(Sender<BrpResponse>);

impl ActiveStreamSender {
    /// Sends a `BrpResult` back to the client.
    ///
    /// `id` is the request ID to associate with the response.
    /// Returns `true` if the message was sent successfully, `false` otherwise (e.g., channel closed).
    fn send(&self, id: Option<Value>, result: BrpResult) -> bool {
        let res = self.0.force_send(BrpResponse::new(id, result));

        match res {
            Ok(Some(_)) => {
                // This occurs if the channel buffer is full.
                warn!(
                    "Channel queue is full, dropping response. Consider increasing the channel size."
                );
            }
            _ => {}
        }

        return res.is_ok();
    }
}

/// Unique identifier for a stream client.
///
/// Typically, this is an incrementing integer.
#[derive(Default, Clone, Copy, Hash, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct StreamClientId(usize);

/// Sets up the MPSC channel for stream messages.
/// This system runs once at `PreStartup`.
fn setup_channel(mut commands: Commands) {
    let (sender, receiver) = channel::bounded(CHANNEL_SIZE);
    commands.insert_resource(StreamSender(sender));
    commands.insert_resource(StreamReceiver(receiver));
}

/// Processes incoming remote requests from the `StreamReceiver`.
///
/// This system runs every `Update`. It handles connect, disconnect, and data messages
/// for streams, invoking the appropriate registered handlers.
fn process_remote_requests(world: &mut World) {
    if !world.contains_resource::<StreamReceiver>() {
        // StreamReceiver might not be initialized yet, or it was removed.
        return;
    }

    // Process all messages currently in the channel.
    while let Ok(stream_message) = world.resource_mut::<StreamReceiver>().try_recv() {
        world.resource_scope(
            |world, methods: Mut<StreamMethods>| match stream_message.kind {
                StreamMessageKind::Connect(req_id, message) => {
                    // Attempt to find the requested stream method.
                    let Some(handler) = methods.0.get(&message.method) else {
                        // Method not found, send an error response.
                        let _ = message.sender.force_send(BrpResponse::new(
                            req_id,
                            Err(BrpError {
                                code: error_codes::METHOD_NOT_FOUND,
                                message: format!("Method `{}` not found", message.method),
                                data: None,
                            }),
                        ));
                        return;
                    };

                    let input = StreamHandlerInput {
                        client_id: stream_message.client_id,
                        params: message.params,
                    };
                    let sender = ActiveStreamSender(message.sender);

                    // Run the on_connect handler if defined.
                    if let Some(on_connect) = handler.on_connect {
                        // If run_handler returns true, it means the stream should be terminated (e.g., error or channel closed).
                        if run_handler(world, on_connect, &input, &sender, req_id.as_ref()) {
                            return; // Stop further processing for this connection attempt.
                        }
                    }

                    // Add the stream to active streams.
                    world.resource_mut::<ActiveStreams>().insert(
                        stream_message.client_id,
                        ActiveStream {
                            request_id: req_id,
                            input,
                            sender,
                            on_update: handler.update,
                            on_disconnect: handler.on_disconnect,
                            on_data: handler.on_data,
                        },
                    );
                }
                StreamMessageKind::Disconnect => {
                    // Remove the stream from active streams.
                    let stream = world
                        .resource_mut::<ActiveStreams>()
                        .remove(&stream_message.client_id);

                    // Run the on_disconnect handler if defined.
                    if let Some(stream) = stream {
                        if let Some(on_disconnect) = stream.on_disconnect {
                            // Note: The result of on_disconnect is ignored as the stream is already being terminated.
                            let _ = world.run_system_with_input(on_disconnect, &stream.input);
                        }
                    }
                }
                StreamMessageKind::Data(value) => {
                    // Process data received on an active stream.
                    world.resource_scope(|world, active_streams: Mut<ActiveStreams>| {
                        let stream = active_streams.get(&stream_message.client_id);

                        let Some(stream) = stream else {
                            // Stream not found, might have been disconnected.
                            return;
                        };

                        // Deserialize the incoming JSON value into a BrpRequest.
                        let request: BrpRequest = match serde_json::from_value(value) {
                            Ok(v) => v,
                            Err(err) => {
                                // Failed to parse, send an error response.
                                stream.sender.send(
                                    None,
                                    Err(BrpError {
                                        code: error_codes::INVALID_REQUEST,
                                        message: format!("Failed to parse request: {err}"),
                                        data: None,
                                    }),
                                );
                                return;
                            }
                        };

                        let Some(on_data) = stream.on_data else {
                            // No on_data handler defined for this stream.
                            return;
                        };

                        let request_id = request.id.clone();
                        // Run the on_data handler.
                        let result = world
                            .run_system_with_input(on_data, (stream_message.client_id, request));

                        match result {
                            Ok(result) => {
                                // If the handler returned Some(result), send it back.
                                let Some(result) = result else {
                                    // Handler returned None, no response to send.
                                    return;
                                };

                                // Only send a response if the original request had an ID (i.e., it's not a notification).
                                if request_id.is_none() {
                                    return;
                                }

                                stream.sender.send(request_id, result);
                            }
                            Err(error) => {
                                // Error running the on_data handler, send an internal error response.
                                stream.sender.send(
                                    request_id,
                                    Err(BrpError {
                                        code: error_codes::INTERNAL_ERROR,
                                        message: format!("Failed to run method handler: {error}"),
                                        data: None,
                                    }),
                                );
                            }
                        }
                    })
                }
            },
        );
    }

    // Process updates for all active streams.
    world.resource_scope(|world, mut streams: Mut<ActiveStreams>| {
        // Collect client IDs of streams that need to be removed.
        let to_remove = streams
            .iter()
            .filter_map(|(client_id, stream)| {
                // Run the update handler for the stream.
                // If run_handler returns true, it signals that the stream should be removed.
                run_handler(
                    world,
                    stream.on_update,
                    &stream.input,
                    &stream.sender,
                    stream.request_id.as_ref(),
                )
                .then_some(*client_id) // If true, map to Some(client_id) for collection.
            })
            .collect::<Vec<_>>();

        // Remove streams marked for removal.
        for client_id in to_remove {
            streams.remove(&client_id);
        }
    });
}

/// Clears all active streams when the application exits.
/// This ensures graceful cleanup of stream resources.
fn on_app_exit(mut active_streams: ResMut<ActiveStreams>) {
    active_streams.clear();
}

/// Runs a given stream handler system and processes its result.
///
/// # Arguments
/// * `world`: Mutable reference to the Bevy `World`.
/// * `system_id`: The `SystemId` of the handler to run.
/// * `input`: The `StreamHandlerInput` for the system.
/// * `sender`: The `ActiveStreamSender` to send responses.
/// * `request_id`: Optional request ID for the response.
///
/// # Returns
/// `true` if the stream should be terminated (due to handler error, handler signaling termination, or channel closure).
/// `false` if the stream should continue.
#[must_use]
fn run_handler(
    world: &mut World,
    system_id: StreamHandler,
    input: &StreamHandlerInput,
    sender: &ActiveStreamSender,
    request_id: Option<&Value>,
) -> bool {
    let result = world.run_system_with_input(system_id, &(input));

    match result {
        Ok(handler_result) => {
            // The handler system executed successfully.
            if let Some(handler_result) = handler_result {
                // The handler returned Some(BrpResult), meaning it wants to send a response or signal an error.
                let handler_err = handler_result.is_err();
                let channel_ok = sender.send(request_id.cloned(), handler_result);

                // Terminate stream if handler returned an error OR if sending the response failed (channel closed).
                handler_err || !channel_ok
            } else {
                // The handler returned None, indicating no action or response is needed from this execution.
                // Stream continues.
                false
            }
        }
        Err(error) => {
            // An error occurred while trying to run the handler system itself.
            sender.send(
                request_id.cloned(),
                Err(BrpError {
                    code: error_codes::INTERNAL_ERROR,
                    message: format!("Failed to run method handler: {error}"),
                    data: None,
                }),
            );
            // Terminate stream due to system execution error.
            true
        }
    }
}
