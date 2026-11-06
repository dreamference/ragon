# Search backends in `ragon-search`

`ragon-web-rs/src/backend.rs` holds the seam: a `SearchBackend` turns a query into the JSON `from_searxng` reads. Two backends exist, and the user's config chooses between them (never a repository's):

```toml
[search]
backends = ["router", "searxng"]
router_url = "http://router.example:8080/route"
```

- With no `[search]` table, or `searxng` first, nothing changes: SearXNG answers.
- `router` first turns the owner's URL router on. It is off by default.

## The URL router (provisional)

The owner delivers and builds the router; this repository builds only the place it plugs into. **The interface below is provisional**, written to the 1.0 scope (§8.1) until the owner states the real one, and changes with it.

- Request: `GET <router_url>?q=<query>`, JSON back: `{"results": [{"url", "title", "snippet"}]}`.
- The router never supplies a URL that is cited unchecked: every returned address (at most ten) is fetched with `ragon-fetch`'s own code and kept only if it answers with a status below 400. Results are labelled with the engine `router`.
- Tried first when on. A router that is unreachable, answers badly, is named but has no `router_url`, or returns nothing that verifies falls back to SearXNG, and the output says so: a `NOTE:` line in the text, a `note` field under `--json` (absent when there is nothing to say).
- Only the general category goes to the router; news, images and the rest go to SearXNG as before.
- Router answers are cached apart from SearXNG's (the backend name is part of the cache key).
- `/airgapped on` refuses it like every backend that uses the network (`refused_at`, checked before the cache).

Still open and tied to the interface: the egress audit knows only loopback ports for a backend (`SEARCH_BACKEND_PORTS`), so a router on another host needs the owner's statement of what it connects to; the MCP server needs no change, it runs `ragon-search`.

Tests: `src/backend.rs` (choice, config), `tests/commands.rs` (a stand-in router and pages: off by default, verification drops an invented address, fallback with its note, air-gap refusal).
