//! The camera URL: read from `SKYLOCK_RTSP_URL`, put into the camera pipeline at run time,
//! and kept out of everything the app prints.

use cu29::prelude::*;

/// Environment variable that holds the camera URL.
pub const URL_VAR: &str = "SKYLOCK_RTSP_URL";
/// Marks where the URL goes in the camera task's `pipeline`.
pub const URL_PLACEHOLDER: &str = "RTSP_URL_PLACEHOLDER";

/// Reads and checks the camera URL. Error messages never include its value.
pub fn url_from_env() -> Result<String, String> {
    match std::env::var(URL_VAR) {
        Ok(value) => check_url(&value),
        Err(std::env::VarError::NotPresent) => Err(format!(
            "{URL_VAR} is not set. Set it to the camera's rtsp:// URL before starting \
             (on Windows, through WINDOWS_SETUP.md section 7)."
        )),
        Err(std::env::VarError::NotUnicode(_)) => Err(format!("{URL_VAR} is not valid Unicode.")),
    }
}

fn check_url(value: &str) -> Result<String, String> {
    let url = value.trim();
    if url.is_empty() {
        return Err(format!("{URL_VAR} is empty."));
    }
    let scheme = url.split_once("://").map(|(scheme, _)| scheme);
    if !scheme.is_some_and(|scheme| scheme.to_ascii_lowercase().starts_with("rtsp")) {
        return Err(format!(
            "{URL_VAR} is not an rtsp:// URL (its value is not shown)."
        ));
    }
    if url.contains('"') || url.chars().any(char::is_whitespace) {
        return Err(format!(
            "{URL_VAR} holds a quote or a space, which a GStreamer pipeline can't take \
             (its value is not shown)."
        ));
    }
    Ok(url.to_string())
}

/// Puts the quoted URL in place of the placeholder in the `pipeline` of task `task`.
pub fn insert_url(config: &mut CuConfig, task: &str, url: &str) -> CuResult<()> {
    let graph = config.get_graph_mut(None)?;
    let node = graph
        .get_node_id_by_name(task)
        .and_then(|id| graph.get_node_mut(id))
        .ok_or_else(|| CuError::from(format!("the config has no task named {task}")))?;
    let pipeline: String = node
        .get_param("pipeline")
        .map_err(|e| CuError::new_with_cause("the camera pipeline is not a string", e))?
        .ok_or_else(|| CuError::from(format!("task {task} has no pipeline")))?;
    if pipeline.matches(URL_PLACEHOLDER).count() != 1 {
        return Err(CuError::from(format!(
            "the pipeline of task {task} must hold {URL_PLACEHOLDER} exactly once"
        )));
    }
    node.set_param(
        "pipeline",
        pipeline.replace(URL_PLACEHOLDER, &format!("\"{url}\"")),
    );
    Ok(())
}

/// Replaces the URL, and the credentials and host inside it, wherever they appear in `text`.
pub fn redact(text: &str, url: &str) -> String {
    let mut out = text.replace(url, "<SKYLOCK_RTSP_URL>");
    let Some((_, rest)) = url.split_once("://") else {
        return out;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    let (userinfo, host_port) = match authority.rsplit_once('@') {
        Some((userinfo, host_port)) => (Some(userinfo), host_port),
        None => (None, authority),
    };
    let host = match host_port.strip_prefix('[') {
        Some(ipv6) => ipv6.split(']').next().unwrap_or(ipv6),
        None => host_port.split(':').next().unwrap_or(host_port),
    };
    let mut secrets = vec![host];
    if let Some(userinfo) = userinfo {
        secrets.push(userinfo);
        secrets.extend(userinfo.split(':'));
    }
    // Longest first, so a user name inside `user:password` doesn't split the pair.
    secrets.sort_by_key(|secret| std::cmp::Reverse(secret.len()));
    for secret in secrets {
        if secret.len() >= 3 {
            out = out.replace(secret, "<redacted>");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "rtsp://viewer:s3cret!@192.0.2.10:554/live/main";

    #[test]
    fn accepts_rtsp_urls_and_trims_them() {
        assert_eq!(check_url(&format!("  {URL}\n")).unwrap(), URL);
        assert!(check_url("rtspt://camera.example/stream").is_ok());
    }

    #[test]
    fn rejects_bad_values_without_echoing_them() {
        for bad in [
            "",
            "   ",
            "http://camera.example/stream",
            "camera.example",
            "rtsp://camera.example/a b",
        ] {
            let message = check_url(bad).unwrap_err();
            assert!(
                bad.trim().is_empty() || !message.contains(bad.trim()),
                "{message}"
            );
        }
        assert!(check_url("rtsp://camera.example/\"x").is_err());
    }

    #[test]
    fn redacts_the_url_and_its_parts() {
        let text =
            format!("could not open {URL}; host 192.0.2.10 refused viewer:s3cret! and s3cret!");
        let out = redact(&text, URL);
        for secret in ["192.0.2.10", "viewer", "s3cret!", "live/main"] {
            assert!(!out.contains(secret), "{secret} survived in: {out}");
        }
        assert!(out.contains("<SKYLOCK_RTSP_URL>"));
    }

    #[test]
    fn inserts_the_quoted_url_once() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/copperconfig.ron");
        let mut config = read_configuration(path).unwrap();
        insert_url(&mut config, "camera", URL).unwrap();

        let graph = config.get_graph_mut(None).unwrap();
        let id = graph.get_node_id_by_name("camera").unwrap();
        let pipeline: String = graph
            .get_node_mut(id)
            .unwrap()
            .get_param("pipeline")
            .unwrap()
            .unwrap();
        assert!(pipeline.contains(&format!("location=\"{URL}\"")));
        assert!(!pipeline.contains(URL_PLACEHOLDER));
        // The placeholder is gone, so a second insert is refused.
        assert!(insert_url(&mut config, "camera", URL).is_err());
        assert!(insert_url(&mut config, "no_such_task", URL).is_err());
    }
}
