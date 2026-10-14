(function () {
  var form = document.getElementById("launch-fields");
  var error = document.getElementById("launch-error");
  var status = document.getElementById("contract-status");
  var host = document.getElementById("host-prompt");
  var participant = document.getElementById("participant-prompt");
  if (!form || !error || !status || !host || !participant) {
    return;
  }

  var fileInput = document.getElementById("file-name");
  var nameInput = document.getElementById("host-name");
  var ephemeralInput = document.getElementById("ephemeral");
  var spec;

  function safeToken(value, pattern) {
    var text = value.trim();
    if (!text || text.length > 120 || !pattern.test(text) || text.indexOf("..") !== -1) {
      return null;
    }
    return text;
  }

  function selectedTransport() {
    var chosen = form.querySelector('input[name="transport"]:checked');
    return chosen ? chosen.value : "loopback";
  }

  function participantText() {
    return (
      "Read the Complexity Eater skill before you edit. " +
      "Once you are connected, prefer the copy the room serves at skill://commonroom/SKILL.md. " +
      "Before that, read " + spec.skill_url + ".\n\n" +
      "Follow the workspace_onboarding prompt:\n\n" +
      spec.onboarding.replace(/\s+$/, "") + "\n\n" +
      "Connect with the local ce binary and leave it running.\n\n" +
      "command: " + spec.paste_command + "\n" +
      "args: " + JSON.stringify(spec.paste_args).replace(/,/g, ", ") + "\n\n" +
      "The invitation works once. Do not send it to this website. " +
      "Propose changes. Approval stays with the workspace owner.\n"
    );
  }

  function hostText(file, name) {
    var serve = selectedTransport() === "tailcat" ? spec.serve_tailcat : spec.serve_loopback;
    serve = serve.split("{file}").join(file).split("{name}").join(name);
    if (ephemeralInput.checked) {
      serve = serve + " " + spec.ephemeral_flag;
    }
    var invite = spec.invite.split("{file}").join(file);
    return (
      "You are on the machine that will host the room. This website did not create it.\n\n" +
      serve + "\n\n" +
      invite + "\n\n" +
      "Give the joining agent the invitation printed by ce and the participant prompt. " +
      "Do not give them the owner URL. A default invitation cannot approve.\n"
    );
  }

  function render() {
    error.textContent = "";
    if (!spec) {
      return;
    }
    var file = safeToken(fileInput.value, /^[A-Za-z0-9._/-]+$/);
    var name = safeToken(nameInput.value, /^[A-Za-z0-9][A-Za-z0-9 ._-]{0,40}$/);
    if (!file || !name) {
      error.textContent = "Use a plain filename and a name without quotes or shell characters.";
      return;
    }
    host.textContent = hostText(file, name);
    participant.textContent = participantText();
  }

  fetch("/agents/first-run.json", { credentials: "same-origin" })
    .then(function (response) {
      if (!response.ok) {
        throw new Error("first-run spec unavailable");
      }
      return response.json();
    })
    .then(function (payload) {
      if (!payload || payload.paste_command !== "ce" || !payload.onboarding) {
        throw new Error("unexpected first-run spec");
      }
      spec = payload;
      status.textContent = "Prompts follow the published skill and room-launch contract version 1. This page did not create a room.";
      render();
    })
    .catch(function () {
      status.textContent = "Showing the default prompts. The first-run spec did not load, and no room was created.";
    });

  form.addEventListener("input", render);
  form.addEventListener("change", render);
})();
