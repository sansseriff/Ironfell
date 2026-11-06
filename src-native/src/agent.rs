use serde_json::{Value, json};

fn connection(
    token: Option<String>,
    endpoint: Option<String>,
    provider: Option<String>,
) -> Result<Option<Value>, String> {
    let Some(token) = token else { return Ok(None) };
    let endpoint = endpoint.unwrap_or_else(|| "http://127.0.0.1:4318".into());
    let provider = provider.unwrap_or_else(|| "claude".into());
    if !(32..=1024).contains(&token.len())
        || endpoint != "http://127.0.0.1:4318"
        || !matches!(provider.as_str(), "claude" | "codex")
    {
        return Err("invalid native agent connection".into());
    }
    Ok(Some(
        json!({ "token": token, "endpoint": endpoint, "provider": provider }),
    ))
}

#[tauri::command]
pub async fn agent_connection(window: tauri::WebviewWindow) -> Result<Option<Value>, String> {
    if std::env::var_os("IRON_NATIVE_AGENT_TEST").is_some() {
        println!("IRON_NATIVE_AGENT_PHASE ipc-enter");
    }
    let url = window.url().map_err(|error| error.to_string())?;
    if std::env::var_os("IRON_NATIVE_AGENT_TEST").is_some() {
        println!("IRON_NATIVE_AGENT_PHASE ipc-window-ready");
    }
    let trusted = (url.scheme() == "tauri" && url.host_str() == Some("localhost"))
        || (matches!(url.scheme(), "http" | "https") && url.host_str() == Some("tauri.localhost"));
    if window.label() != "main" || !trusted {
        return Err("native agent connection is only available to the local main window".into());
    }
    connection(
        std::env::var("IRON_NATIVE_AGENT_TOKEN").ok(),
        std::env::var("IRON_NATIVE_AGENT_ENDPOINT").ok(),
        std::env::var("IRON_NATIVE_AGENT_PROVIDER").ok(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_connection_requires_a_valid_launcher_token() {
        assert!(connection(None, None, None).unwrap().is_none());
        assert!(connection(Some("short".into()), None, None).is_err());
        assert!(
            connection(
                Some("x".repeat(48)),
                Some("https://example.com".into()),
                None
            )
            .is_err()
        );
        assert!(connection(Some("x".repeat(48)), None, Some("shell".into())).is_err());
        let result = connection(Some("x".repeat(48)), None, None)
            .unwrap()
            .unwrap();
        assert_eq!(result["provider"], "claude");
        assert_eq!(result["endpoint"], "http://127.0.0.1:4318");
    }
}
