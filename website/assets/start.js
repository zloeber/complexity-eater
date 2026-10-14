(function () {
  var form = document.getElementById("launch-fields");
  var error = document.getElementById("launch-error");
  var status = document.getElementById("contract-status");
  if (!form || !error || !status) {
    return;
  }

  var fileInput = document.getElementById("file-name");
  var nameInput = document.getElementById("host-name");
  var agentInput = document.getElementById("agent-name");
  var ephemeralInput = document.getElementById("ephemeral");
  var contract;

  function safeToken(value, pattern, fallback) {
    var text = value.trim();
    if (!text || text.length > 120 || !pattern.test(text) || text.indexOf("..") !== -1) {
      return null;
    }
    return text || fallback;
  }

  function selectedTransport() {
    var chosen = form.querySelector('input[name="transport"]:checked');
    return chosen ? chosen.value : "loopback";
  }

  function apply(template, file, name, agent) {
    return template.split("{file}").join(file).split("{name}").join(name).split("{agent}").join(agent);
  }

  function fill(id, text) {
    var node = document.getElementById(id);
    if (node) {
      node.textContent = text;
    }
  }

  function render() {
    error.textContent = "";
    if (!contract) {
      return;
    }
    var file = safeToken(fileInput.value, /^[A-Za-z0-9._/-]+$/, "notes.md");
    var name = safeToken(nameInput.value, /^[A-Za-z0-9][A-Za-z0-9 ._-]{0,40}$/, "Ada");
    var agent = safeToken(agentInput.value, /^[A-Za-z0-9][A-Za-z0-9 ._-]{0,40}$/, "Agent");
    if (!file || !name || !agent) {
      error.textContent = "Use a plain filename and names without quotes or shell characters.";
      return;
    }
    var local = contract.modes.local;
    var transport = local.transports[selectedTransport()] || local.transports.loopback;
    var serve = apply(transport.command, file, name, agent);
    if (ephemeralInput.checked) {
      serve += " " + local.ephemeral.flag;
    }
    fill("cmd-serve", serve);
    fill("cmd-invite", apply(local.invite_command, file, name, agent));
    fill("cmd-join", apply(local.join_command, file, name, agent));
    fill("cmd-mcp", apply(local.mcp_command, file, name, agent));
    fill(
      "cmd-paste",
      "command: " + local.agent_paste.command + "\nargs: " + JSON.stringify(local.agent_paste.args)
    );
    var reach = transport.remote_participants
      ? "Remote participants can dial this room when the tailcat plugin is running."
      : "This room is local to the machine running ce. A remote participant cannot use it.";
    fill("transport-reach", reach);
  }

  fetch("/contracts/room-launch.v1.json", { credentials: "same-origin" })
    .then(function (response) {
      if (!response.ok) {
        throw new Error("contract unavailable");
      }
      return response.json();
    })
    .then(function (payload) {
      if (payload.version !== "1" || payload.provisions_room !== false) {
        throw new Error("unexpected contract");
      }
      if (!payload.modes || payload.modes.hosted.status !== "not_implemented") {
        throw new Error("hosted mode is not available");
      }
      contract = payload;
      status.textContent = "Commands follow room-launch contract version 1. This page did not create a room.";
      render();
    })
    .catch(function () {
      status.textContent = "Showing the default local commands. The launch contract did not load, and no room was created.";
    });

  form.addEventListener("input", render);
  form.addEventListener("change", render);
})();
