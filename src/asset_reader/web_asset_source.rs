use bevy::{asset::io::PathStream, tasks::ConditionalSendFuture};
use std::path::{Path, PathBuf};

use bevy::asset::io::{AssetReader, AssetReaderError, Reader};

/// Treats paths as urls to load assets from.
pub enum WebAssetReader {
    /// Unencrypted connections.
    Http,
    /// Use TLS for setting up connections.
    Https,
}

impl WebAssetReader {
    fn make_uri(&self, path: &Path) -> PathBuf {
        PathBuf::from(match self {
            Self::Http => "http://",
            Self::Https => "https://",
        })
        .join(path)
    }

    /// See [bevy::asset::io::get_meta_path]
    fn make_meta_uri(&self, path: &Path) -> Option<PathBuf> {
        let mut uri = self.make_uri(path);
        let mut extension = path.extension()?.to_os_string();
        extension.push(".meta");
        uri.set_extension(extension);
        Some(uri)
    }
}

#[cfg(target_arch = "wasm32")]
async fn get(path: PathBuf) -> Result<Box<dyn Reader>, AssetReaderError> {
    use bevy::asset::io::VecReader;
    use js_sys::{Uint8Array, global};
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;
    use web_sys::{Request, RequestInit, Response};

    fn js_value_to_err<'a>(
        context: &'a str,
    ) -> impl FnOnce(wasm_bindgen::JsValue) -> std::io::Error + 'a {
        move |value| {
            let message = match js_sys::JSON::stringify(&value) {
                Ok(js_str) => format!("Failed to {context}: {js_str}"),
                Err(_) => {
                    format!(
                        "Failed to {context} and also failed to stringify the JSValue of the error"
                    )
                }
            };

            std::io::Error::new(std::io::ErrorKind::Other, message)
        }
    }

    // Create a fetch request using the global fetch function that works in both the main thread and workers
    let mut opts = RequestInit::new();
    opts.set_method("GET");

    let request = Request::new_with_str_and_init(path.to_str().unwrap(), &opts)
        .map_err(js_value_to_err("create request"))?;

    // Use the global fetch function (works in both window and worker contexts)
    let global = global();
    let resp_promise = js_sys::Reflect::get(&global, &"fetch".into())
        .map_err(js_value_to_err("get fetch function"))?
        .dyn_into::<js_sys::Function>()
        .map_err(js_value_to_err("cast to function"))?
        .call1(&global, &request.into())
        .map_err(js_value_to_err("call fetch"))?;

    let resp_value = JsFuture::from(
        resp_promise
            .dyn_into::<js_sys::Promise>()
            .map_err(js_value_to_err("cast promise"))?,
    )
    .await
    .map_err(js_value_to_err("fetch path"))?;

    let resp = resp_value
        .dyn_into::<Response>()
        .map_err(js_value_to_err("convert fetch to Response"))?;

    match resp.status() {
        200 => {
            let array_buffer = JsFuture::from(
                resp.array_buffer()
                    .map_err(js_value_to_err("get array buffer"))?,
            )
            .await
            .map_err(js_value_to_err("await array buffer"))?;

            let bytes = Uint8Array::new(&array_buffer).to_vec();
            let reader: Box<dyn Reader> = Box::new(VecReader::new(bytes));
            Ok(reader)
        }
        404 => Err(AssetReaderError::NotFound(path)),
        status => Err(AssetReaderError::Io(
            std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Encountered unexpected HTTP status {status}"),
            )
            .into(),
        )),
    }
}

#[cfg(not(target_arch = "wasm32"))]
async fn get(path: PathBuf) -> Result<Box<dyn Reader>, AssetReaderError> {
    use std::future::Future;
    use std::io;
    use std::pin::Pin;
    use std::task::{Context, Poll};

    use bevy::asset::io::VecReader;
    use surf::StatusCode;

    #[pin_project::pin_project]
    struct ContinuousPoll<T>(#[pin] T);

    impl<T: Future> Future for ContinuousPoll<T> {
        type Output = T::Output;

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            // Always wake - blocks on single threaded executor.
            cx.waker().wake_by_ref();

            self.project().0.poll(cx)
        }
    }

    let str_path = path.to_str().ok_or_else(|| {
        AssetReaderError::Io(
            io::Error::new(
                io::ErrorKind::Other,
                format!("non-utf8 path: {}", path.display()),
            )
            .into(),
        )
    })?;

    #[cfg(not(feature = "redirect"))]
    let client = surf::Client::new();

    #[cfg(feature = "redirect")]
    let client = surf::Client::new().with(surf::middleware::Redirect::default());

    let mut response = ContinuousPoll(client.get(str_path)).await.map_err(|err| {
        AssetReaderError::Io(
            io::Error::new(
                io::ErrorKind::Other,
                format!(
                    "unexpected status code {} while loading {}: {}",
                    err.status(),
                    path.display(),
                    err.into_inner(),
                ),
            )
            .into(),
        )
    })?;

    match response.status() {
        StatusCode::Ok => Ok(Box::new(VecReader::new(
            ContinuousPoll(response.body_bytes())
                .await
                .map_err(|_| AssetReaderError::NotFound(path.to_path_buf()))?,
        )) as _),
        StatusCode::NotFound => Err(AssetReaderError::NotFound(path)),
        code => Err(AssetReaderError::Io(
            io::Error::new(
                io::ErrorKind::Other,
                format!(
                    "unexpected status code {} while loading {}",
                    code,
                    path.display()
                ),
            )
            .into(),
        )),
    }
}

impl AssetReader for WebAssetReader {
    fn read<'a>(
        &'a self,
        path: &'a Path,
    ) -> impl ConditionalSendFuture<Output = Result<Box<dyn Reader>, AssetReaderError>> {
        get(self.make_uri(path))
    }

    async fn read_meta<'a>(&'a self, path: &'a Path) -> Result<Box<dyn Reader>, AssetReaderError> {
        match self.make_meta_uri(path) {
            Some(uri) => get(uri).await,
            None => Err(AssetReaderError::NotFound(
                "source path has no extension".into(),
            )),
        }
    }

    async fn is_directory<'a>(&'a self, _path: &'a Path) -> Result<bool, AssetReaderError> {
        Ok(false)
    }

    async fn read_directory<'a>(
        &'a self,
        path: &'a Path,
    ) -> Result<Box<PathStream>, AssetReaderError> {
        Err(AssetReaderError::NotFound(self.make_uri(path)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn make_http_uri() {
        assert_eq!(
            WebAssetReader::Http
                .make_uri(Path::new("s3.johanhelsing.studio/dump/favicon.png"))
                .to_str()
                .unwrap(),
            "http://s3.johanhelsing.studio/dump/favicon.png"
        );
    }

    #[test]
    fn make_https_uri() {
        assert_eq!(
            WebAssetReader::Https
                .make_uri(Path::new("s3.johanhelsing.studio/dump/favicon.png"))
                .to_str()
                .unwrap(),
            "https://s3.johanhelsing.studio/dump/favicon.png"
        );
    }

    #[test]
    fn make_http_meta_uri() {
        assert_eq!(
            WebAssetReader::Http
                .make_meta_uri(Path::new("s3.johanhelsing.studio/dump/favicon.png"))
                .expect("cannot create meta uri")
                .to_str()
                .unwrap(),
            "http://s3.johanhelsing.studio/dump/favicon.png.meta"
        );
    }

    #[test]
    fn make_https_meta_uri() {
        assert_eq!(
            WebAssetReader::Https
                .make_meta_uri(Path::new("s3.johanhelsing.studio/dump/favicon.png"))
                .expect("cannot create meta uri")
                .to_str()
                .unwrap(),
            "https://s3.johanhelsing.studio/dump/favicon.png.meta"
        );
    }

    #[test]
    fn make_https_without_extension_meta_uri() {
        assert_eq!(
            WebAssetReader::Https.make_meta_uri(Path::new("s3.johanhelsing.studio/dump/favicon")),
            None
        );
    }
}
