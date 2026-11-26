// The Account page (AUTH §12, §14.4). Every value from the service is put in with textContent.
(function () {
  "use strict";
  var TOKEN = "ragon.account.token";
  var REFRESH = "ragon.account.refresh";
  var $ = function (id) { return document.getElementById(id); };
  var me = null;

  function say(text, bad) {
    var s = $("status");
    s.textContent = text || "";
    s.className = bad ? "error" : "";
  }

  function api(method, path, body) {
    var headers = { Authorization: "Bearer " + (localStorage.getItem(TOKEN) || "") };
    var init = { method: method, headers: headers };
    if (body !== undefined) {
      headers["Content-Type"] = "application/json";
      init.body = JSON.stringify(body);
    }
    return fetch(path, init).then(function (r) {
      return r.text().then(function (t) {
        var data = null;
        try { data = t ? JSON.parse(t) : null; } catch (e) { data = null; }
        if (!r.ok) {
          var msg = data && data.error && (data.error.message || data.error);
          var err = new Error(typeof msg === "string" ? msg : "Request failed (" + r.status + ").");
          err.status = r.status;
          throw err;
        }
        return data;
      });
    });
  }

  function list(data, name) {
    if (Array.isArray(data)) return data;
    return (data && data[name]) || [];
  }

  function cell(row, value) {
    var td = document.createElement("td");
    td.textContent = value === undefined || value === null ? "" : String(value);
    row.appendChild(td);
    return td;
  }

  function when(seconds) {
    return seconds ? new Date(seconds * 1000).toLocaleString() : "";
  }

  function button(label, action) {
    var b = document.createElement("button");
    b.type = "button";
    b.textContent = label;
    b.addEventListener("click", function () {
      Promise.resolve(action()).then(refresh, function (e) { say(e.message, true); });
    });
    return b;
  }

  function fill(id, rows) {
    var body = $(id);
    while (body.firstChild) body.removeChild(body.firstChild);
    rows.forEach(function (r) { body.appendChild(r); });
  }

  function showIdentity() {
    var rows = [["Username", me.username], ["Email", me.email], ["Role", me.role], ["Node", me.primary_name]];
    fill("identity-rows", rows.map(function (pair) {
      var tr = document.createElement("tr");
      var th = document.createElement("th");
      th.textContent = pair[0];
      tr.appendChild(th);
      cell(tr, pair[1]);
      return tr;
    }));
  }

  function showSessions(data) {
    fill("sessions", list(data, "sessions").map(function (s) {
      var tr = document.createElement("tr");
      cell(tr, s.machine || "");
      cell(tr, (s.client_id || "") + (s.current ? " (this browser)" : ""));
      cell(tr, when(s.last_use));
      var td = document.createElement("td");
      td.appendChild(button("Revoke", function () {
        return api("DELETE", "/ragon/v1/users/me/sessions/" + encodeURIComponent(s.id));
      }));
      tr.appendChild(td);
      return tr;
    }));
  }

  function showKeys(data) {
    fill("keys", list(data, "keys").map(function (k) {
      var tr = document.createElement("tr");
      cell(tr, k.name);
      cell(tr, when(k.created));
      cell(tr, when(k.last_use));
      var td = document.createElement("td");
      td.appendChild(button("Revoke", function () {
        return api("DELETE", "/ragon/v1/users/me/keys/" + encodeURIComponent(k.name));
      }));
      tr.appendChild(td);
      return tr;
    }));
  }

  function showBindings(data) {
    fill("bindings", list(data, "bindings").map(function (b) {
      var tr = document.createElement("tr");
      cell(tr, b.kind || "");
      cell(tr, b.label || b.chat || b.id || "");
      var td = document.createElement("td");
      td.appendChild(button("Remove", function () {
        return api("DELETE", "/ragon/v1/bindings/" + encodeURIComponent(b.id));
      }));
      tr.appendChild(td);
      return tr;
    }));
  }

  function showUsage(data) {
    fill("usage", list(data, "usage").map(function (u) {
      var tr = document.createElement("tr");
      cell(tr, u.day);
      cell(tr, u.username || (me && me.username) || "");
      cell(tr, u.requests);
      cell(tr, (u.input_tokens || 0) + (u.output_tokens || 0) || u.tokens || 0);
      return tr;
    }));
  }

  function showUsers(data) {
    fill("users", list(data, "users").map(function (u) {
      var tr = document.createElement("tr");
      cell(tr, u.username + (u.service ? " (service)" : ""));
      cell(tr, u.role);
      var td = document.createElement("td");
      var base = "/ragon/v1/users/" + encodeURIComponent(u.username);
      td.appendChild(button("Reset", function () {
        return api("POST", base + "/reset").then(function (r) {
          var pw = r && (r.password || r.one_time_password);
          $("reset-result").textContent = pw ? "One-time password for " + u.username + ": " + pw : "Password reset for " + u.username + ".";
        });
      }));
      td.appendChild(button(u.role === "admin" ? "Demote" : "Promote", function () {
        return api("POST", base + (u.role === "admin" ? "/demote" : "/promote"));
      }));
      td.appendChild(button("Remove", function () {
        if (!window.confirm("Remove " + u.username + "?")) return null;
        return api("DELETE", base);
      }));
      tr.appendChild(td);
      return tr;
    }));
  }

  function optional(path, show) {
    return api("GET", path).then(show, function (e) {
      if (e.status !== 404 && e.status !== 501) throw e;
      show(null);
    });
  }

  function refresh() {
    return api("GET", "/ragon/v1/users/me").then(function (profile) {
      me = profile;
      $("signin").hidden = true;
      $("signed-in").hidden = false;
      $("admin").hidden = me.role !== "admin";
      showIdentity();
      say("");
      var jobs = [
        optional("/ragon/v1/users/me/sessions", showSessions),
        api("GET", "/ragon/v1/users/me/keys").then(showKeys),
        optional("/ragon/v1/bindings", showBindings),
        optional(me.role === "admin" ? "/ragon/v1/usage" : "/ragon/v1/users/me/usage", showUsage)
      ];
      if (me.role === "admin") jobs.push(optional("/ragon/v1/users", showUsers));
      return Promise.all(jobs);
    }).catch(function (e) {
      if (e.status === 401) {
        localStorage.removeItem(TOKEN);
        $("signed-in").hidden = true;
        $("signin").hidden = false;
        say("Sign in with your Ragon account.");
      } else {
        say(e.message, true);
      }
    });
  }

  function showKeyOnce(result) {
    var key = result && (result.key || result.api_key || result.secret) || "";
    var node = (me.primary_name || "node") + ".local";
    $("key-value").textContent = key;
    $("snippet-config").textContent =
      'model_provider = "ragon"\n[model_providers.ragon]\nname = "Ragon"\nbase_url = "https://' + node +
      ':8000/v1"\nwire_api = "responses"\nenv_key = "RAGON_API_KEY"';
    $("snippet-env").textContent =
      "export OPENAI_BASE_URL=https://" + node + ":8000/v1\nexport OPENAI_API_KEY=" + key;
    $("snippet-ca").textContent =
      "export SSL_CERT_FILE=<the node's CA file>\nexport REQUESTS_CA_BUNDLE=<the node's CA file>\nexport NODE_EXTRA_CA_CERTS=<the node's CA file>";
    $("key-once").hidden = false;
  }

  $("signin").addEventListener("submit", function (ev) {
    ev.preventDefault();
    var form = new URLSearchParams({
      grant_type: "password",
      username: $("username").value,
      password: $("password").value
    });
    fetch("/oauth/token", {
      method: "POST",
      headers: { "Content-Type": "application/x-www-form-urlencoded" },
      body: form.toString()
    }).then(function (r) {
      return r.json().then(function (d) { return { ok: r.ok, data: d }; });
    }).then(function (r) {
      $("password").value = "";
      if (!r.ok || !r.data.access_token) {
        say(r.data.error_description || "Sign in failed.", true);
        return null;
      }
      localStorage.setItem(TOKEN, r.data.access_token);
      if (r.data.refresh_token) localStorage.setItem(REFRESH, r.data.refresh_token);
      return refresh();
    }).catch(function (e) { say(e.message, true); });
  });

  $("signout").addEventListener("click", function () {
    var refresh_token = localStorage.getItem(REFRESH);
    var done = function () {
      localStorage.removeItem(TOKEN);
      localStorage.removeItem(REFRESH);
      location.reload();
    };
    if (!refresh_token) return done();
    fetch("/oauth/revoke", {
      method: "POST",
      headers: { "Content-Type": "application/x-www-form-urlencoded" },
      body: new URLSearchParams({ token: refresh_token }).toString()
    }).then(done, done);
  });

  $("revoke-all").addEventListener("click", function () {
    api("DELETE", "/ragon/v1/users/me/sessions").then(refresh, function (e) { say(e.message, true); });
  });

  $("new-key").addEventListener("submit", function (ev) {
    ev.preventDefault();
    var name = $("key-name").value.trim();
    api("POST", "/ragon/v1/users/me/keys", { name: name }).then(function (result) {
      showKeyOnce(result);
      $("key-name").value = "";
      return refresh();
    }).catch(function (e) { say(e.message, true); });
  });

  $("pair-phone").addEventListener("click", function () {
    api("POST", "/ragon/v1/bindings", {}).then(function (r) {
      var code = r && (r.code || r.pairing_code);
      $("pairing-code").textContent = code ? "Send this code to the bot from your phone: " + code : "Pairing started.";
      return refresh();
    }).catch(function (e) { say(e.message, true); });
  });

  $("password-form").addEventListener("submit", function (ev) {
    ev.preventDefault();
    api("POST", "/ragon/v1/users/me/password", { current: $("pw-current").value, "new": $("pw-new").value }).then(function () {
      $("pw-current").value = "";
      $("pw-new").value = "";
      say("Password changed.");
    }, function (e) { say(e.message, true); });
  });

  $("delete-form").addEventListener("submit", function (ev) {
    ev.preventDefault();
    if (!me || $("delete-confirm").value !== me.username) {
      say("Type your username to confirm.", true);
      return;
    }
    api("DELETE", "/ragon/v1/users/me").then(function () {
      localStorage.removeItem(TOKEN);
      localStorage.removeItem(REFRESH);
      location.reload();
    }, function (e) { say(e.message, true); });
  });

  refresh();
})();
