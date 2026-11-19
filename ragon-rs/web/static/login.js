// The sign-in page of ragon web (AUTH §5.8, §14.4): the password grant through this server, the
// JWT kept in local storage until logout, the device code and Forgejo's authorization step.
(function () {
  "use strict";
  var KEY = "ragon.session";
  var path = location.pathname;
  var query = new URLSearchParams(location.search);
  var $ = function (id) { return document.getElementById(id); };

  function saved() {
    try { return JSON.parse(localStorage.getItem(KEY) || "null"); } catch (e) { return null; }
  }
  function say(text, bad) {
    var box = $("message");
    box.textContent = text;
    box.className = bad ? "bad" : "ok";
  }
  function post(url, body, token, method) {
    var headers = { "Content-Type": "application/json" };
    if (token) headers.Authorization = "Bearer " + token;
    return fetch(url, { method: method || "POST", headers: headers, body: body ? JSON.stringify(body) : undefined })
      .then(function (r) { return r.json().catch(function () { return {}; }).then(function (j) { return { ok: r.ok, body: j }; }); });
  }
  function store(set) {
    var session = { access_token: set.access_token, refresh_token: set.refresh_token, at: Date.now() };
    localStorage.setItem(KEY, JSON.stringify(session));
    return post("/api/auth/session", { refresh_token: set.refresh_token }, set.access_token).then(function () { return session; });
  }
  function fresh() {
    var s = saved();
    if (!s || !s.refresh_token) return Promise.resolve(null);
    return post("/api/auth/refresh", { refresh_token: s.refresh_token }).then(function (r) {
      if (!r.ok) { localStorage.removeItem(KEY); return null; }
      return store(r.body);
    });
  }
  function show(id) {
    ["signin", "device", "authorize", "done"].forEach(function (name) { $(name).hidden = name !== id; });
  }
  function afterSignIn() {
    if (path === "/device") { show("device"); return; }
    if (path === "/oauth/authorize") { show("authorize"); return; }
    show("done");
  }
  function signIn(create) {
    post("/api/auth/login", { username: $("username").value, password: $("password").value, create: !!create }).then(function (r) {
      if (!r.ok) { say(r.body.error_description || "That did not work.", true); return; }
      store(r.body).then(function () { say("", false); afterSignIn(); });
    });
  }

  $("do-signin").addEventListener("click", function () { signIn(false); });
  $("do-create").addEventListener("click", function () { signIn(true); });
  $("do-logout").addEventListener("click", function () {
    var s = saved();
    localStorage.removeItem(KEY);
    if (s) post("/api/auth/session", null, s.access_token, "DELETE");
    say("Signed out.", false);
    show("signin");
  });
  $("do-approve").addEventListener("click", function () {
    var s = saved();
    if (!s) { show("signin"); return; }
    post("/api/auth/device", { user_code: $("user-code").value.replace(/[\s-]/g, "").toUpperCase() }, s.access_token).then(function (r) {
      say(r.ok ? "Approved. You can go back to the other device." : (r.body.error_description || "That code was not accepted."), !r.ok);
    });
  });
  $("do-allow").addEventListener("click", function () {
    var s = saved();
    if (!s) { show("signin"); return; }
    var body = {};
    query.forEach(function (value, name) { body[name] = value; });
    post("/api/auth/authorize", body, s.access_token).then(function (r) {
      if (r.ok && r.body.redirect_to) { location.href = r.body.redirect_to; return; }
      say(r.body.error_description || "That was not allowed.", true);
    });
  });
  if (query.get("user_code")) $("user-code").value = query.get("user_code");

  fresh().then(function (s) { if (s) afterSignIn(); else show("signin"); });
})();
