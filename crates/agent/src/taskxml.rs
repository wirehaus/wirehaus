use std::path::{Path, PathBuf};

pub const TASK_NAME: &str = "Wirehaus Agent";
const RETRY_INTERVAL: &str = "PT1M";
const FIRST_START: &str = "2000-01-01T00:00:00";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskConfig {
    pub launcher: PathBuf,
    pub state_dir: PathBuf,
    pub viewer_dir: PathBuf,
    pub listen: std::net::SocketAddr,
    pub public_hosts: Vec<String>,
    pub viewer_origins: Vec<String>,
    pub user_sid: String,
    pub working_dir: PathBuf,
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            other => out.push(other),
        }
    }
    out
}

pub fn quote(argument: &str) -> String {
    format!("\"{}\"", argument.replace('"', "\\\""))
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

pub fn arguments(config: &TaskConfig) -> String {
    let mut items = vec![
        "--state-dir".to_owned(),
        quote(&path_text(&config.state_dir)),
        "serve".to_owned(),
        "--viewer-dir".to_owned(),
        quote(&path_text(&config.viewer_dir)),
        "--listen".to_owned(),
        config.listen.to_string(),
    ];
    for host in &config.public_hosts {
        items.push("--public-host".to_owned());
        items.push(quote(host));
    }
    for origin in &config.viewer_origins {
        items.push("--viewer-origin".to_owned());
        items.push(quote(origin));
    }
    items.join(" ")
}

pub fn task_xml(config: &TaskConfig) -> String {
    let user = escape(&config.user_sid);
    let lines = [
        "<?xml version=\"1.0\" encoding=\"UTF-16\"?>".to_owned(),
        "<Task version=\"1.4\" xmlns=\"http://schemas.microsoft.com/windows/2004/02/mit/task\">"
            .to_owned(),
        "  <RegistrationInfo>".to_owned(),
        "    <Description>Wirehaus agent</Description>".to_owned(),
        "  </RegistrationInfo>".to_owned(),
        "  <Triggers>".to_owned(),
        "    <LogonTrigger>".to_owned(),
        "      <Enabled>true</Enabled>".to_owned(),
        format!("      <UserId>{user}</UserId>"),
        "    </LogonTrigger>".to_owned(),
        "    <TimeTrigger>".to_owned(),
        "      <Repetition>".to_owned(),
        format!("        <Interval>{RETRY_INTERVAL}</Interval>"),
        "        <StopAtDurationEnd>false</StopAtDurationEnd>".to_owned(),
        "      </Repetition>".to_owned(),
        format!("      <StartBoundary>{FIRST_START}</StartBoundary>"),
        "      <Enabled>true</Enabled>".to_owned(),
        "    </TimeTrigger>".to_owned(),
        "  </Triggers>".to_owned(),
        "  <Principals>".to_owned(),
        "    <Principal id=\"Author\">".to_owned(),
        format!("      <UserId>{user}</UserId>"),
        "      <LogonType>InteractiveToken</LogonType>".to_owned(),
        "      <RunLevel>LeastPrivilege</RunLevel>".to_owned(),
        "    </Principal>".to_owned(),
        "  </Principals>".to_owned(),
        "  <Settings>".to_owned(),
        "    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>".to_owned(),
        "    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>".to_owned(),
        "    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>".to_owned(),
        "    <AllowHardTerminate>true</AllowHardTerminate>".to_owned(),
        "    <StartWhenAvailable>true</StartWhenAvailable>".to_owned(),
        "    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>".to_owned(),
        "    <Enabled>true</Enabled>".to_owned(),
        "  </Settings>".to_owned(),
        "  <Actions Context=\"Author\">".to_owned(),
        "    <Exec>".to_owned(),
        format!(
            "      <Command>{}</Command>",
            escape(&path_text(&config.launcher))
        ),
        format!(
            "      <Arguments>{}</Arguments>",
            escape(&arguments(config))
        ),
        format!(
            "      <WorkingDirectory>{}</WorkingDirectory>",
            escape(&path_text(&config.working_dir))
        ),
        "    </Exec>".to_owned(),
        "  </Actions>".to_owned(),
        "</Task>".to_owned(),
    ];
    let mut text = lines.join("\r\n");
    text.push_str("\r\n");
    text
}

pub fn utf16_with_bom(text: &str) -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xFE];
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes
}

pub fn create_arguments(xml: &Path) -> Vec<String> {
    vec![
        "/Create".to_owned(),
        "/TN".to_owned(),
        TASK_NAME.to_owned(),
        "/XML".to_owned(),
        path_text(xml),
        "/F".to_owned(),
    ]
}

pub fn run_arguments() -> Vec<String> {
    vec!["/Run".to_owned(), "/TN".to_owned(), TASK_NAME.to_owned()]
}

pub fn end_arguments() -> Vec<String> {
    vec!["/End".to_owned(), "/TN".to_owned(), TASK_NAME.to_owned()]
}

pub fn delete_arguments() -> Vec<String> {
    vec![
        "/Delete".to_owned(),
        "/TN".to_owned(),
        TASK_NAME.to_owned(),
        "/F".to_owned(),
    ]
}

pub fn query_arguments() -> Vec<String> {
    vec!["/Query".to_owned(), "/TN".to_owned(), TASK_NAME.to_owned()]
}

pub fn tasklist_arguments(image: &str) -> Vec<String> {
    vec![
        "/FI".to_owned(),
        format!("IMAGENAME eq {image}"),
        "/FO".to_owned(),
        "CSV".to_owned(),
        "/NH".to_owned(),
    ]
}

pub fn kill_arguments(image: &str, own_pid: u32) -> Vec<String> {
    vec![
        "/F".to_owned(),
        "/FI".to_owned(),
        format!("IMAGENAME eq {image}"),
        "/FI".to_owned(),
        format!("PID ne {own_pid}"),
    ]
}

pub fn process_listed(tasklist: &str, image: &str) -> bool {
    let wanted = format!("\"{image}\"");
    tasklist.lines().any(|line| {
        line.trim_start()
            .to_lowercase()
            .starts_with(&wanted.to_lowercase())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> TaskConfig {
        TaskConfig {
            launcher: PathBuf::from(r"C:\Program Files\Wirehaus\wirehaus-agentd.exe"),
            state_dir: PathBuf::from(r"C:\Users\Ana Pérez\AppData\Roaming\Wirehaus"),
            viewer_dir: PathBuf::from(r"C:\Program Files\Wirehaus\viewer"),
            listen: "127.0.0.1:7878".parse().unwrap(),
            public_hosts: Vec::new(),
            viewer_origins: Vec::new(),
            user_sid: "S-1-5-21-1-2-3-1001".to_owned(),
            working_dir: PathBuf::from(r"C:\Program Files\Wirehaus"),
        }
    }

    #[test]
    fn the_task_runs_at_logon_and_retries_every_minute_without_stacking() {
        let text = task_xml(&config());
        assert!(text.contains("<LogonTrigger>"));
        assert!(text.contains("<Interval>PT1M</Interval>"));
        assert!(text.contains("<MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>"));
        assert!(text.contains("<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>"));
        assert!(text.contains("<DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>"));
    }

    #[test]
    fn the_task_belongs_to_the_interactive_user_and_never_runs_elevated() {
        let text = task_xml(&config());
        assert!(text.contains("<LogonType>InteractiveToken</LogonType>"));
        assert!(text.contains("<RunLevel>LeastPrivilege</RunLevel>"));
        assert_eq!(
            text.matches("<UserId>S-1-5-21-1-2-3-1001</UserId>").count(),
            2
        );
        assert!(!text.contains("HighestAvailable"));
    }

    #[test]
    fn the_public_host_and_viewer_origin_are_quoted_arguments() {
        let mut settings = config();
        settings.public_hosts = vec!["ana.wirehaus.dev".to_owned()];
        settings.viewer_origins = vec!["https://app.wirehaus.dev".to_owned()];
        assert!(arguments(&settings).ends_with(
            " --public-host \"ana.wirehaus.dev\" --viewer-origin \"https://app.wirehaus.dev\""
        ));
    }

    #[test]
    fn paths_with_spaces_are_quoted_and_the_listener_stays_on_loopback() {
        assert_eq!(
            arguments(&config()),
            "--state-dir \"C:\\Users\\Ana Pérez\\AppData\\Roaming\\Wirehaus\" serve --viewer-dir \"C:\\Program Files\\Wirehaus\\viewer\" --listen 127.0.0.1:7878"
        );
    }

    #[test]
    fn markup_in_a_path_cannot_break_out_of_the_xml() {
        let mut config = config();
        config.viewer_dir = PathBuf::from("C:\\a&b<c>");
        let text = task_xml(&config);
        assert!(text.contains("a&amp;b&lt;c&gt;"));
        assert!(!text.contains("<c>"));
    }

    #[test]
    fn the_xml_file_is_utf16_with_a_byte_order_mark() {
        let bytes = utf16_with_bom("<a>é</a>");
        assert_eq!(&bytes[..2], &[0xFF, 0xFE]);
        assert_eq!(bytes.len(), 2 + 2 * "<a>é</a>".chars().count());
        assert_eq!(&bytes[2..4], &[b'<', 0]);
    }

    #[test]
    fn stopping_the_agent_never_targets_the_process_doing_the_stopping() {
        assert_eq!(
            kill_arguments("wirehaus-agent.exe", 4242),
            [
                "/F",
                "/FI",
                "IMAGENAME eq wirehaus-agent.exe",
                "/FI",
                "PID ne 4242"
            ]
        );
    }

    #[test]
    fn a_running_agent_is_recognised_in_the_tasklist_output() {
        let listed = "\"wirehaus-agent.exe\",\"1234\",\"Console\",\"1\",\"12.000 KB\"\r\n";
        assert!(process_listed(listed, "wirehaus-agent.exe"));
        assert!(!process_listed(
            "INFORMACIÓN: no hay tareas en ejecución que coincidan con los criterios especificados.",
            "wirehaus-agent.exe"
        ));
        assert!(!process_listed("\"other.exe\",\"1\"", "wirehaus-agent.exe"));
    }
}
