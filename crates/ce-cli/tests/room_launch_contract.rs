use serde_json::Value;

fn contract() -> Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../website/contracts/room-launch.v1.json");
    let text = std::fs::read_to_string(&path).expect("room-launch contract");
    serde_json::from_str(&text).expect("room-launch contract json")
}

fn cli_commands() -> Vec<String> {
    let source = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/cli.rs"))
        .expect("cli source");
    let start = source
        .find("const COMMANDS: &[&str] = &[")
        .expect("command list");
    let end = source[start..].find("];").expect("command list end") + start;
    source[start..end]
        .lines()
        .filter_map(|line| {
            let line = line.trim().trim_matches(',').trim();
            line.strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
                .map(str::to_string)
        })
        .collect()
}

fn assert_command(template: &str, commands: &[String]) {
    assert!(
        !template.chars().any(|ch| "\n;&|$`".contains(ch)),
        "command template must be one command: {template}"
    );
    let tokens: Vec<&str> = template.split_whitespace().collect();
    assert_eq!(tokens[0], "ce");
    let second = tokens[1];
    if !"{{'<".contains(second.chars().next().unwrap()) {
        assert!(
            commands.iter().any(|command| command == second),
            "{second} is not a ce command"
        );
    }
    let flags = [
        "--name",
        "--tailcat",
        "--workspace",
        "--json",
        "--no-hold",
        "--invite",
        "--ephemeral",
    ];
    for token in tokens {
        if token.starts_with("--") {
            assert!(flags.contains(&token), "unexpected flag {token}");
        }
    }
}

#[test]
fn room_launch_contract_matches_the_local_cli() {
    let value = contract();
    assert_eq!(value["contract"], "commonroom.room-launch");
    assert_eq!(value["version"], "1");
    assert_eq!(value["transmits_workspace"], false);
    assert_eq!(value["provisions_room"], false);
    assert_eq!(value["modes"]["hosted"]["status"], "not_implemented");
    assert_eq!(value["modes"]["hosted"]["provisions_room"], false);
    assert!(value["modes"]["hosted"]["provider_endpoint"].is_null());

    let local = &value["modes"]["local"];
    assert_eq!(local["status"], "available");
    assert_eq!(local["invitation_prefix"], "commonroom://join/");
    assert_eq!(local["invitation_uses"], 1);
    assert_eq!(local["failed_dial_spends_invitation"], false);
    assert_eq!(local["host"]["process"], "ce");
    assert_eq!(local["host"]["binds"], "127.0.0.1");
    assert_eq!(local["host"]["owner_port_cookie"], "commonroom_session");
    assert_eq!(local["host"]["mcp_path"], "/mcp");
    assert_eq!(local["host"]["mcp_revision"], "2026-07-28");
    assert_eq!(
        local["transports"]["loopback"]["remote_participants"],
        false
    );
    assert_eq!(local["transports"]["tailcat"]["remote_participants"], true);
    assert_eq!(
        local["transports"]["tailcat"]["missing_plugin"],
        "transport_unavailable"
    );
    assert_eq!(local["ephemeral"]["flag"], "--ephemeral");
    assert_eq!(local["agent_paste"]["command"], "ce");
    assert_eq!(local["agent_paste"]["args"][0], "mcp");
    assert_eq!(local["agent_paste"]["args"][1], "--invite");
    assert_eq!(
        local["default_capabilities"],
        serde_json::json!(["read", "observe", "propose", "checkout", "commit"])
    );
    assert_eq!(
        local["withheld_capabilities"],
        serde_json::json!(["approve", "invite", "admin"])
    );

    let commands = cli_commands();
    for template in [
        local["host"]["equivalent"][0].as_str().unwrap(),
        local["host"]["equivalent"][1].as_str().unwrap(),
        local["transports"]["loopback"]["command"].as_str().unwrap(),
        local["transports"]["tailcat"]["command"].as_str().unwrap(),
        local["invite_command"].as_str().unwrap(),
        local["join_command"].as_str().unwrap(),
        local["mcp_command"].as_str().unwrap(),
    ] {
        assert_command(template, &commands);
    }
}
