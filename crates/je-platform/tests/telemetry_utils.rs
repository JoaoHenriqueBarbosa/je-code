//! Porte de `src/vs/platform/telemetry/test/common/telemetryUtils.test.ts` (suíte `TelemetryUtils`), caso a caso e na mesma ordem.
//!
//! Divergências: o `TestConfigurationService` é uma função de leitura de configuração (o `getTelemetryLevel`
//! recebe `&dyn Fn(&str) -> Dynamic`); o `getValue` que devolve `null` é uma função que devolve `Dynamic::Null`.
//! `config` com objetos vazios (`{}`) são listas de nomes (só a presença da chave importa a `cleanRemoteAuthority`).

mod common;

use common::*;
use je_base::dynamic::Dynamic;
use je_platform::telemetry::{RemoteAuthoringConfig, TelemetryConfiguration, TelemetryLevel, clean_data, clean_remote_authority, get_telemetry_level};

fn level(setting: Option<Dynamic>) -> TelemetryLevel {
    get_telemetry_level(&|key| match (&setting, key) {
        (Some(value), "telemetry.telemetryLevel") => value.clone(),
        _ => Dynamic::Undefined,
    })
}

#[test]
fn get_telemetry_level_fails_closed_for_invalid_configuration() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let configured = |c: TelemetryConfiguration| level(Some(s(c.as_str())));
    assert_eq!(
        vec![
            level(None),
            configured(TelemetryConfiguration::On),
            configured(TelemetryConfiguration::Error),
            configured(TelemetryConfiguration::Crash),
            configured(TelemetryConfiguration::Off),
            level(Some(s("invalid"))),
            get_telemetry_level(&|_| Dynamic::Null),
        ],
        vec![
            TelemetryLevel::Usage,
            TelemetryLevel::Usage,
            TelemetryLevel::Error,
            TelemetryLevel::Crash,
            TelemetryLevel::None,
            TelemetryLevel::None,
            TelemetryLevel::None,
        ]
    );
}

mod clean_data_suite {
    use super::*;

    #[test]
    fn redacts_command_line_credentials_and_shortened_api_keys() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let placeholder = "PLACEHOLDER";
        let input = obj(vec![
            ("shortenedGoogleApiKey", s(&format!("\"maps\": \"AIza{}\",", "A1b2C3d4E5".repeat(3)))),
            ("plinkPassword", s(&format!("plink.exe -ssh -batch -l some-user -pw '{placeholder}' build-host \"uptime\""))),
            ("openSslPassword", s(&format!("openssl enc -aes-256-cbc -pass pass:{placeholder} -in notes.txt"))),
            ("windowsPassword", s(&format!("runas /user:admin /pass:{placeholder} app.exe"))),
            ("sshpassPassword", s(&format!("sshpass -p {placeholder} ssh deploy@buildbox"))),
            ("curlBasicAuth", s(&format!("curl -sS -u admin:{placeholder} api.internal:8080"))),
            ("curlAttachedBasicAuth", s(&format!("curl -sS -uadmin:{placeholder} api.internal:8080"))),
            ("curlShortPassword", s("curl -sS -u admin:x api.internal:8080")),
        ]);
        let expected = obj(vec![
            ("shortenedGoogleApiKey", s("<REDACTED: Google API Key>")),
            ("plinkPassword", s("<REDACTED: CLI Password Flag>")),
            ("openSslPassword", s("<REDACTED: CLI Password Flag>")),
            ("windowsPassword", s("<REDACTED: CLI Password Flag>")),
            ("sshpassPassword", s("<REDACTED: CLI Password Flag>")),
            ("curlBasicAuth", s("<REDACTED: CLI Basic Auth>")),
            ("curlAttachedBasicAuth", s("<REDACTED: CLI Basic Auth>")),
            ("curlShortPassword", s("<REDACTED: CLI Basic Auth>")),
        ]);
        assert!(je_base::objects::equals(&clean_data(Some(&input), &[], None, &[]), &expected));
    }

    #[test]
    fn does_not_redact_common_command_line_flags() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let commands = obj(vec![
            ("mkdir", s("mkdir -p src && git log -p -1")),
            ("ssh", s("ssh -p 2222 deploy@buildbox")),
            ("docker", s("docker run -u 1000:1000 -p 8080:80 nginx")),
            ("dockerAttached", s("docker exec -u0:0 container sh")),
            ("msbuild", s("dotnet build /p:Configuration=Release /p:Platform=x64")),
            ("jest", s("npx jest --passWithNoTests --ci")),
            ("git", s("git push -u origin main")),
        ]);
        assert!(je_base::objects::equals(&clean_data(Some(&commands), &[], None, &[]), &commands));
    }

    #[test]
    fn removes_one_drive_organization_names_from_paths() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let input = obj(vec![
            ("path", s("Set-Location 'C:\\Users\\someone\\OneDrive - Contoso Corporation\\Documents\\repo'; git status")),
            ("pathWithApostrophe", s("Set-Location 'C:\\Users\\someone\\OneDrive - O'Brien Holdings\\Documents\\repo'; git status")),
            ("text", s("Could not sync \"OneDrive - Contoso Corporation\" folder")),
        ]);
        let expected = obj(vec![
            ("path", s("Set-Location '<REDACTED: user-file-path>'; git status")),
            ("pathWithApostrophe", s("Set-Location '<REDACTED: user-file-path>'; git status")),
            ("text", s("Could not sync \"OneDrive\" folder")),
        ]);
        assert!(je_base::objects::equals(&clean_data(Some(&input), &[], None, &[]), &expected));
    }
}

fn config(remote: Option<&[&str]>, virtual_workspace: Option<&[&str]>) -> RemoteAuthoringConfig {
    let names = |list: Option<&[&str]>| list.map(|l| l.iter().map(|n| (*n).to_string()).collect());
    RemoteAuthoringConfig { remote_extension_tips: names(remote), virtual_workspace_extension_tips: names(virtual_workspace) }
}

mod clean_remote_authority_suite {
    use super::*;

    #[test]
    fn returns_none_when_remote_authority_is_undefined() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let config = config(Some(&["ssh-remote"]), Some(&["codespaces"]));
        assert_eq!(clean_remote_authority(None, Some(&config)), "none");
    }

    #[test]
    fn returns_remote_name_when_it_exists_in_remote_extension_tips() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let config = config(Some(&["ssh-remote", "dev-container", "wsl"]), None);
        assert_eq!(clean_remote_authority(Some("ssh-remote"), Some(&config)), "ssh-remote");
        assert_eq!(clean_remote_authority(Some("dev-container"), Some(&config)), "dev-container");
        assert_eq!(clean_remote_authority(Some("wsl"), Some(&config)), "wsl");
    }

    #[test]
    fn returns_remote_name_when_it_exists_in_virtual_workspace_extension_tips() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let config = config(Some(&[]), Some(&["codespaces", "tunnel"]));
        assert_eq!(clean_remote_authority(Some("codespaces"), Some(&config)), "codespaces");
        assert_eq!(clean_remote_authority(Some("tunnel"), Some(&config)), "tunnel");
    }

    #[test]
    fn returns_other_when_remote_name_is_not_in_either_config() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let config = config(Some(&["ssh-remote", "dev-container"]), Some(&["codespaces"]));
        assert_eq!(clean_remote_authority(Some("unknown-remote"), Some(&config)), "other");
        assert_eq!(clean_remote_authority(Some("custom-remote"), Some(&config)), "other");
    }

    #[test]
    fn returns_other_when_config_is_empty() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let config = config(Some(&[]), Some(&[]));
        assert_eq!(clean_remote_authority(Some("ssh-remote"), Some(&config)), "other");
    }

    #[test]
    fn handles_config_with_undefined_remote_extension_tips() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let config = config(None, Some(&["codespaces"]));
        assert_eq!(clean_remote_authority(Some("codespaces"), Some(&config)), "codespaces");
        assert_eq!(clean_remote_authority(Some("ssh-remote"), Some(&config)), "other");
    }

    #[test]
    fn handles_config_with_undefined_virtual_workspace_extension_tips() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let config = config(Some(&["ssh-remote"]), None);
        assert_eq!(clean_remote_authority(Some("ssh-remote"), Some(&config)), "ssh-remote");
        assert_eq!(clean_remote_authority(Some("codespaces"), Some(&config)), "other");
    }

    #[test]
    fn handles_empty_config_object() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let config = config(None, None);
        assert_eq!(clean_remote_authority(Some("ssh-remote"), Some(&config)), "other");
        assert_eq!(clean_remote_authority(None, Some(&config)), "none");
    }

    #[test]
    fn handles_remote_authority_with_additional_path_segments() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let config = config(Some(&["ssh-remote"]), None);
        // getRemoteName should extract just the authority name
        assert_eq!(clean_remote_authority(Some("ssh-remote+server1.example.com"), Some(&config)), "ssh-remote");
    }

    #[test]
    fn handles_undefined_config_object() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_eq!(clean_remote_authority(Some("ssh-remote"), None), "other");
        assert_eq!(clean_remote_authority(None, None), "none");
    }
}
