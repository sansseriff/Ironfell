use bevy::prelude::*;

use crate::asset_reader::web_asset_source::*;
use bevy::asset::io::AssetSourceBuilder;

/// Add this plugin to bevy to support loading http and https urls.
///
/// Needs to be added before Bevy's `DefaultPlugins`.
///
/// # Example
///
/// ```no_run
/// # use bevy::prelude::*;
/// # use bevy_web_asset::WebAssetPlugin;
///
/// let mut app = App::new();
///
/// app.add_plugins((
///     WebAssetPlugin::default(),
///     DefaultPlugins
/// ));
/// ```
#[derive(Default)]
pub struct WebAssetPlugin;

impl Plugin for WebAssetPlugin {
    fn build(&self, app: &mut App) {
        // bevy 0.18 removed `AssetSource::build()`; the builder now takes its reader
        // up front via `AssetSourceBuilder::new`.
        app.register_asset_source(
            "http",
            AssetSourceBuilder::new(|| Box::new(WebAssetReader::Http)),
        );
        app.register_asset_source(
            "https",
            AssetSourceBuilder::new(|| Box::new(WebAssetReader::Https)),
        );
    }
}
