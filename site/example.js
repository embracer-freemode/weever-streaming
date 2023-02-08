// examples

// the entry point of Weever Streaming demo examples
function form_submit(event) {
  let form = event.srcElement;
  let data = new FormData(form);

  let settings = {
    room: data.get("room"),
    id: data.get("id"),
    token: data.get("token"),
    url: ".",
    constraints: null,
    debug: true,
  }

  // cache for weever streaming clients for later clean up
  let pub_default;
  let pub_screen;
  let sub;

  // hide the form
  form.style.display = "none";

  // show bottom bar
  document.getElementById("utils").style.display = "";

  // hook "leave" button in bottom bar
  document.getElementById("leave").onclick = () => {
    form.style.display = "";
    document.getElementById("utils").style.display = "none";
    document.getElementById("sub-media").textContent = "";
    document.getElementById("pub-media").textContent = "";
    // close connection
    [sub, pub_default, pub_screen].forEach((client) => {
      if (client) {
        try {
          sub.close();
        } catch (error) {
          console.log(error);
        }
      }
    });
  }

  // hook "toggle video" button in bottom bar
  document.getElementById("camera").onclick = () => {
    // the publisher client
    let client = pub_default;
    client.pc.getSenders().forEach((sender) => {
      if (sender.track.kind == "video") {
        let state = !sender.track.enabled;
        sender.track.enabled = state;
        if (state) {
          document.getElementById("cameraOn").style.display = "";
          document.getElementById("cameraOff").style.display = "none";
        } else {
          document.getElementById("cameraOn").style.display = "none";
          document.getElementById("cameraOff").style.display = "";
        }
      }
    });
  }

  // hook "toggle audio" button in bottom bar
  document.getElementById("microphone").onclick = () => {
    // the publisher client
    let client = pub_default;
    client.pc.getSenders().forEach((sender) => {
      if (sender.track.kind == "audio") {
        let state = !sender.track.enabled;
        sender.track.enabled = state;
        if (state) {
          document.getElementById("microphoneOn").style.display = "";
          document.getElementById("microphoneOff").style.display = "none";
        } else {
          document.getElementById("microphoneOn").style.display = "none";
          document.getElementById("microphoneOff").style.display = "";
        }
      }
    });
  }

  // hook "toggle screen share" button in bottom bar
  document.getElementById("screen").onclick = () => {
    let state = document.getElementById("screenOn").style.display == "";
    if (state) {
      // start screen share
      settings.constraints = { audio: false, video: true }
      pub_screen = example_pub(settings, screen=true);
      document.getElementById("screenOn").style.display = "none";
      document.getElementById("screenOff").style.display = "";
    } else {
      // the publisher client
      let client = pub_screen;
      client.close();
      // stop screen share
      document.getElementById("screenOn").style.display = "";
      document.getElementById("screenOff").style.display = "none";
      // remove video
      document.getElementById(`pub-media-${client.id}-screen`).remove();
    }
  }

  // detect use case selection for different settings
  let useCase = data.get("useCase");
  // 1, Video Conferencing
  if (useCase == 1) {
    // enable both pub/sub
    sub = example_sub(settings);
    settings.constraints = { audio: true, video: true }
    pub_default = example_pub(settings);
    // show "toggle video" button
    document.getElementById("camera").style.display = "";
    // show "screen share" button
    document.getElementById("screen").style.display = "";
    // show "toggle audio" button
    document.getElementById("microphone").style.display = "";
  }
  // 2, Audio Chat
  else if (useCase == 2) {
    // enable both pub/sub
    // disable video in pub
    sub = example_sub(settings);
    settings.constraints = { audio: true, video: false }
    pub_default = example_pub(settings);
    // hide "toggle video" button
    document.getElementById("camera").style.display = "none";
    // hide "screen share" button
    document.getElementById("screen").style.display = "none";
    // show "toggle audio" button
    document.getElementById("microphone").style.display = "";
  }
  // 3, Broadcasting
  else if (useCase == 3) {
    let role = data.get("broadcastingRole");
    // 1, Broadcaster
    if (role == 1) {
      sub = example_sub(settings);
      settings.constraints = { audio: Boolean(data.get("enableAudio")), video: Boolean(data.get("enableVideo")) }
      pub_default = example_pub(settings);
      // hide/show "toggle video" button
      document.getElementById("camera").style.display = data.get("enableVideo") ? "" : "none";
      // hide/show "screen share" button
      document.getElementById("screen").style.display = data.get("enableVideo") ? "" : "none";
      // hide/show "toggle audio" button
      document.getElementById("microphone").style.display = data.get("enableAudio") ? "" : "none";
    }
    // 2, Viewer/Listener
    else if (role == 2) {
      sub = example_sub(settings);
      // hide "toggle video" button
      document.getElementById("camera").style.display = "none";
      // hide "screen share" button
      document.getElementById("screen").style.display =  "none";
      // hide "toggle audio" button
      document.getElementById("microphone").style.display = "none";
    }
  }
  // 4, (Raw) Publisher
  else if (useCase == 4) {
    // enable pub only
    settings.constraints = { audio: Boolean(data.get("enableAudio")), video: Boolean(data.get("enableVideo")) }
    pub_default = example_pub(settings);
    // hide/show "toggle video" button
    document.getElementById("camera").style.display = data.get("enableVideo") ? "" : "none";
    // hide/show "scree share" button
    document.getElementById("screen").style.display = data.get("enableVideo") ? "" : "none";
    // hide/show "toggle audio" button
    document.getElementById("microphone").style.display = data.get("enableAudio") ? "" : "none";
  }
  // 5, (Raw) Subscriber
  else if (useCase == 5) {
    // enable sub only
    sub = example_sub(settings);
    // hide "toggle video" button
    document.getElementById("camera").style.display = "none";
    // hide "screen share" button
    document.getElementById("screen").style.display =  "none";
    // hide "toggle audio" button
    document.getElementById("microphone").style.display = "none";
  }

  // don't really submit anything
  return false;
}

function _notify(msg) {
  let notification = document.createElement("div");
  // HTML template for a notification
  notification.innerHTML = `
    <div class="toast bg-info" role="alert" aria-live="assertive" aria-atomic="true">
      <div class="toast-header">
        <strong class="me-auto">Weever Streaming</strong>
        <small class="text-muted">just now</small>
        <button type="button" class="btn-close" data-bs-dismiss="toast" aria-label="Close"></button>
      </div>
      <div class="toast-body text-dark">
        ${msg}
      </div>
    </div>
    `.trim();
  notification = notification.firstChild;
  let toast = new bootstrap.Toast(notification);
  document.getElementById("notification").appendChild(notification);
  toast.show();
}

// example for running subscriber with UI change
function example_sub(settings) {
  let client = new WeeverPeerConnection();

  function update_layout() {
    let all = document.getElementById("sub-media");
    let width = Math.floor(all.childElementCount/2) + 1;
    client._log(`set media layout width to ${width}`);
    all.style.gridTemplateAreas = "'" + Array.from("a".repeat(width)).join(" ") + "'";
  }

  client.setUrl(settings.url);
  client.setRoom(settings.room);
  client.setId(settings.id);
  client.setToken(settings.token);
  client.onSubStream = (stream, id, full_id, event) => {
    var elem;
    elem = document.getElementById(`sub-media-${full_id}`)
    if (elem == null) {
      elem = document.createElement("div")
      // HTML template for publisher media
      elem.innerHTML = `
        <div id="sub-media-${full_id}">
            ${id}
            <video style="width: 30%"></video>
            <audio style=""></audio>
        </div>
        `.trim();
      elem = elem.firstChild;
      document.getElementById("sub-media").appendChild(elem)
    }

    // the kind can be video or audio
    let media_id = `sub-media-${id}-${event.track.kind}-${event.track.id}`;
    let media = elem.getElementsByTagName(event.track.kind)[0];
    media.id = media_id;
    media.srcObject = event.streams[0]
    media.autoplay = true
    media.controls = true

    update_layout();
  };
  client.onPubJoin = (id, full_id) => {
    _notify(`Publisher ${id} Joined.`);
  };
  client.onPubLeft = (id, full_id) => {
    _notify(`Publisher ${id} Left.`);

    div = document.getElementById(`sub-media-${full_id}`);
    if (div != null) {
      div.remove();
    }

    update_layout();
  };
  client.setDebug(settings.debug);
  client._log(JSON.stringify(settings));
  client.onError = (error) => {
    _notify(`Got error: ${error}`);
    document.getElementById("leave").click();
  };
  client.subscribe();
  return client;
}

// example for running publisher with UI change
function example_pub(settings, screen=false) {
  document.getElementById("pub-media").style.display = "";
  let client = new WeeverPeerConnection();
  client.setUrl(settings.url);
  client.setRoom(settings.room);
  client.setId(settings.id);
  client.setToken(settings.token);
  client.setDebug(settings.debug);
  client.onPubStream = stream => {
    let id_suffix = screen ? "screen" : "default";
    let str_suffix = screen ? "'s screen" : "";
    let elem = document.createElement("div")
    // HTML template for publisher local media
    elem.innerHTML = `
      <div id="pub-media-${client.id}-${id_suffix}">
          ${client.id}${str_suffix}
          <video style="width: 30%"></video>
          <audio></audio>
      </div>
      `.trim();
    elem = elem.firstChild;
    let media = elem.getElementsByTagName("video")[0];
    media.srcObject = stream;
    media.autoplay = true
    media.controls = true
    document.getElementById("pub-media").appendChild(elem)
  };
  client._log(JSON.stringify(settings));
  client.onError = (error) => {
    _notify(`Got error: ${error}`);
    document.getElementById("leave").click();
  };
  if (screen) {
    // it's screen share
    client.publish_screen(settings.constraints);
  } else {
    // it's not screen share
    client.publish(settings.constraints);
  }
  return client;
}
