use bevy::prelude::*;
use bevy::render::view::RenderLayers;
use bevy::render::camera::RenderTarget;
use bevy::window::WindowRef;
use crate::canvas_view::CanvasName;
use bevy_vello::prelude::*;

/// Timeline window plugin for managing timeline view with 2D graphics rendering
pub struct TimelinePlugin;

impl Plugin for TimelinePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TimelineState>()
            .add_systems(Update, (
                setup_timeline_window.run_if(timeline_not_setup),
                update_timeline_view, 
                render_timeline_grid
            ));
    }
}

/// Condition to check if timeline is not set up yet
fn timeline_not_setup(
    timeline_camera: Query<&TimelineCamera2D>,
) -> bool {
    timeline_camera.is_empty()
}

/// Resource to manage timeline state and configuration
#[derive(Resource, Debug)]
pub struct TimelineState {
    pub zoom: f64,
    pub offset: f64,
    pub duration: f64,
    pub current_time: f64,
    pub playing: bool,
}

impl Default for TimelineState {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            offset: 0.0,
            duration: 30.0, // 30 seconds default
            current_time: 0.0,
            playing: false,
        }
    }
}

/// Marker component for timeline camera
#[derive(Component)]
pub struct TimelineCamera2D;

/// Marker component to identify the timeline window
#[derive(Component)]
pub struct TimelineWindow;

/// Marker component to identify the primary/viewer window  
#[derive(Component)]
pub struct ViewerWindow;

/// Marker component for timeline background scene
#[derive(Component)]
pub struct TimelineBackgroundScene;

/// Marker component for timeline grid scene
#[derive(Component)]
pub struct TimelineGridScene;

/// Marker component for timeline playhead scene
#[derive(Component)]
pub struct TimelinePlayheadScene;

/// Setup the timeline window with 2D camera and initial scenes
pub fn setup_timeline_window(
    mut commands: Commands,
    windows: Query<(Entity, Option<&CanvasName>), With<Window>>,
) {
    // Wait for windows to be created by canvas system
    let window_entities: Vec<(Entity, Option<&CanvasName>)> = windows.iter().collect();
    
    // if window_entities.len() < 2 {
    //     // Not enough windows created yet, will retry next frame
    //     warn!("Waiting for windows to be created. Found: {}, Expected: 2+", window_entities.len());
    //     return;
    // }

    // // Sort window entities by their titles to identify them correctly
    // // Primary window will have default title, Timeline window has "Timeline Window"
    // let mut sorted_windows: Vec<(Entity, String)> = window_entities.iter()
    //     .filter_map(|&entity| {
    //         window_query.get(entity).ok()
    //             .map(|window| (entity, window.title.clone()))
    //     })
    //     .collect();
    
    // // Sort: Primary window (not "Timeline Window") first, then Timeline window
    // sorted_windows.sort_by(|a, b| {
    //     if a.1 != "Timeline Window" && b.1 == "Timeline Window" {
    //         std::cmp::Ordering::Less    // Primary window comes first
    //     } else if a.1 == "Timeline Window" && b.1 != "Timeline Window" {
    //         std::cmp::Ordering::Greater // Timeline window comes second
    //     } else {
    //         std::cmp::Ordering::Equal
    //     }
    // });
    
    // if sorted_windows.len() < 2 {
    //     warn!("Could not identify window titles properly");
    //     return;
    // }
    
    // Try to pick explicit windows by CanvasName
    let mut _viewer_window: Option<Entity> = None;
    let mut timeline_window: Option<Entity> = None;
    for (e, name) in window_entities.iter() {
        if let Some(CanvasName(id)) = name {
            if id == "viewer-canvas" { _viewer_window = Some(*e); }
            if id == "timeline-canvas" { timeline_window = Some(*e); }
        }
    }

    // If the timeline window isn't ready yet, wait until it exists
    let Some(timeline_window) = timeline_window else {
        info!("Timeline setup waiting for timeline window entity (found {} windows)", window_entities.len());
        return;
    };
    
    // // Mark the windows so we can identify them
    // commands.entity(viewer_window).insert(ViewerWindow);
    // commands.entity(timeline_window).insert(TimelineWindow);
    
    // info!("Timeline window setup: viewer={:?} ({}), timeline={:?} ({})", 
    //       viewer_window, sorted_windows[0].1, timeline_window, sorted_windows[1].1);

    // Setup timeline 2D camera - will render to timeline window
    let camera_target = RenderTarget::Window(WindowRef::Entity(timeline_window));
        commands.spawn((
            Camera2d,
            Camera {
                order: 1,
                clear_color: ClearColorConfig::Custom(Color::srgb(0.15, 1.0, 0.19)), 
                target: camera_target,
                ..default()
            },
            RenderLayers::layer(2), // Different render layer for timeline
            TimelineCamera2D,
            VelloView,
        ));

    // Timeline background scene
    commands.spawn((
        VelloScene::new(),
        RenderLayers::layer(2),
        TimelineBackgroundScene,
    ));

    // Timeline grid scene for time markers and grid lines
    commands.spawn((
        VelloScene::new(),
        RenderLayers::layer(2),
        TimelineGridScene,
    ));

    // Timeline playhead scene (current time indicator)
    commands.spawn((
        VelloScene::new(),
        RenderLayers::layer(2),
        TimelinePlayheadScene,
    ));

    info!("Timeline window setup complete");
}

/// Update timeline view based on current state
pub fn update_timeline_view(
    mut timeline: ResMut<TimelineState>,
    time: Res<Time>,
) {
    // Update current time if playing
    if timeline.playing {
        timeline.current_time += time.delta_secs_f64();
        if timeline.current_time > timeline.duration {
            timeline.current_time = timeline.duration;
            timeline.playing = false; // Stop at end
        }
    }
}

/// Render the timeline grid with time markers
pub fn render_timeline_grid(
    mut grid_scene: Query<&mut VelloScene, With<TimelineGridScene>>,
    mut playhead_scene: Query<
        &mut VelloScene,
        (With<TimelinePlayheadScene>, Without<TimelineGridScene>),
    >,
    timeline: Res<TimelineState>,
) {
    // Render grid
    if let Ok(mut scene) = grid_scene.single_mut() {
        scene.reset();

        // Timeline dimensions (adjust based on your needs)
        let timeline_width: f64 = 800.0;
        let timeline_height: f64 = 100.0;
        let timeline_left: f64 = -timeline_width / 2.0;
        let timeline_right: f64 = timeline_width / 2.0;
        let timeline_top: f64 = timeline_height / 2.0;
        let timeline_bottom: f64 = -timeline_height / 2.0;

        // Draw timeline background
        let background_rect = kurbo::Rect::new(
            timeline_left,
            timeline_bottom,
            timeline_right,
            timeline_top,
        );
        scene.fill(
            peniko::Fill::NonZero,
            kurbo::Affine::default(),
            peniko::Color::new([1.0, 1.0, 0.1, 1.0]), // Bright yellow background
            None,
            &background_rect,
        );

        // Draw time grid lines
        let time_per_pixel: f64 = timeline.duration / timeline_width;
        let major_step: f64 = 5.0; // Major grid line every 5 seconds
        let minor_step: f64 = 1.0; // Minor grid line every 1 second

        // Major grid lines
        let mut time: f64 = 0.0;
        while time <= timeline.duration {
            let x: f64 = timeline_left + (time / time_per_pixel);
            let line = kurbo::Line::new((x, timeline_bottom), (x, timeline_top));
            
            if (time % major_step).abs() < 0.01 {
                // Major line - thicker and brighter
                scene.stroke(
                    &kurbo::Stroke::new(2.0),
                    kurbo::Affine::default(),
                    peniko::Color::new([0.5, 0.5, 0.5, 1.0]),
                    None,
                    &line,
                );
            } else if (time % minor_step).abs() < 0.01 {
                // Minor line - thinner and darker
                scene.stroke(
                    &kurbo::Stroke::new(1.0),
                    kurbo::Affine::default(),
                    peniko::Color::new([0.3, 0.3, 0.3, 1.0]),
                    None,
                    &line,
                );
            }
            
            time += 0.5; // Check every 0.5 seconds for grid lines
        }
    }

    // Render playhead
    if let Ok(mut scene) = playhead_scene.single_mut() {
        scene.reset();

        let timeline_width: f64 = 800.0;
        let timeline_height: f64 = 100.0;
        let timeline_left: f64 = -timeline_width / 2.0;
        let timeline_top: f64 = timeline_height / 2.0;
        let timeline_bottom: f64 = -timeline_height / 2.0;

        // Calculate playhead position
        let time_per_pixel: f64 = timeline.duration / timeline_width;
        let playhead_x: f64 = timeline_left + (timeline.current_time / time_per_pixel);

        // Draw playhead line
        let playhead_line = kurbo::Line::new(
            (playhead_x, timeline_bottom),
            (playhead_x, timeline_top),
        );
        scene.stroke(
            &kurbo::Stroke::new(3.0),
            kurbo::Affine::default(),
            peniko::Color::new([1.0, 0.2, 0.2, 1.0]), // Red playhead
            None,
            &playhead_line,
        );

        // Draw playhead handle (triangle at top)
        let handle_size = 8.0;
        let mut handle_path = kurbo::BezPath::new();
        handle_path.move_to((playhead_x, timeline_top));
        handle_path.line_to((playhead_x - handle_size, timeline_top + handle_size));
        handle_path.line_to((playhead_x + handle_size, timeline_top + handle_size));
        handle_path.close_path();

        scene.fill(
            peniko::Fill::NonZero,
            kurbo::Affine::default(),
            peniko::Color::new([1.0, 0.2, 0.2, 1.0]), // Red playhead handle
            None,
            &handle_path,
        );
    }
}