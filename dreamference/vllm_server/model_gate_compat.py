"""What the model gate answers for an upstream Codex (specs/DREAMFERENCE_AUTH.md §5.9, §10).

Copied beside `model_gate_service.py` into the gate's container and imported by it, so it uses the
standard library only. Everything here is a pure function of its arguments except `origins`, which
reads one file from the gate's state folder:

- the model list in the API's shape and in Codex's `?client_version=` shape, with the aliases an
  upstream Codex sends (`models`, `alias_target`, `unknown_model`);
- the `x-codex-*` rate-limit headers and request ids of a `/v1/responses` answer (`limit_headers`);
- the `/backend-api/*` stubs and the vendor's `unknown_url` 404 (`backend`);
- the parameters the engine rejects, removed with a note (`sanitize`);
- the error body in the vendor's shape (`error`);
- CORS for the node's `ragon web` origins and the desktop app only (`cors_headers`).
"""

import base64
import json
import time
import uuid
from pathlib import Path
from typing import Any, Final

# The names an upstream Codex sends for its default model, besides the served id (§5.9).
ALIASES: Final[tuple[str, ...]] = ("ragon", "ragon-default", "gpt-6-astra")
ALIAS_OWNER: Final[str] = "ragon"
MODELS_PATH: Final[str] = "/v1/models"
CODEX_QUERY: Final[str] = "client_version="
BACKEND_PREFIX: Final[str] = "/backend-api"

# Sent by the launcher's provider only in the engine's own shape, so the gate drops them: the
# engine keeps no responses, no store and no per-user state (§5.9).
DROPPED_PARAMS: Final[tuple[str, ...]] = (
    "store",
    "metadata",
    "service_tier",
    "user",
    "prompt_cache_key",
    "safety_identifier",
    "previous_response_id",
    "include",
)
DROPPED_REASONING: Final[tuple[str, ...]] = ("summary",)
SANITIZED_PATHS: Final[tuple[str, ...]] = (
    "/v1/responses",
    "/v1/chat/completions",
    "/v1/completions",
)

# Codex's usage windows: five hours and seven days (`codex-api/src/rate_limits.rs`).
PRIMARY_WINDOW_MINUTES: Final[int] = 300
SECONDARY_WINDOW_MINUTES: Final[int] = 10080
SECONDS_PER_MINUTE: Final[int] = 60
DESKTOP_ORIGIN: Final[str] = "app://-"
ORIGINS_FILE: Final[str] = "web-origins.json"
CORS_METHODS: Final[str] = "GET, POST, DELETE, OPTIONS"
CORS_HEADERS: Final[str] = "authorization, content-type, api-key, x-ragon-job"
CORS_MAX_AGE: Final[str] = "600"
HTTP_NOT_FOUND: Final[int] = 404
HEADER_SEP: Final[bytes] = b":"
WILDCARD: Final[str] = "*"
JWT_PARTS: Final[int] = 3
BEARER: Final[str] = "bearer "

# What a stub answers for each `/backend-api/` endpoint of the §5.9 table (the route is matched on
# its tail, so `wham/usage` and `api/codex/usage` both answer).
USAGE_TAILS: Final[tuple[str, ...]] = ("wham/usage", "api/codex/usage")
EMPTY_LIST_TAILS: Final[tuple[str, ...]] = (
    "workspace-messages",
    "connectors",
    "connectors/directory/list",
    "plugins",
    "plugins/list",
    "plugins/featured",
    "plugins/installed",
)
EMPTY_SETTINGS_TAILS: Final[tuple[str, ...]] = ("settings/user",)
ACCOUNT_TAILS: Final[tuple[str, ...]] = ("accounts/check", "wham/accounts/check")
PROFILE_TAILS: Final[tuple[str, ...]] = ("profiles/me",)
# Answered with the 404 shape on purpose: cloud tasks, the config bundle, thread usage, analytics.
SHAPE_404_TAILS: Final[tuple[str, ...]] = (
    "wham/tasks",
    "wham/config",
    "wham/threads",
    "analytics-events",
)


class ModelGateCompat:
    """The gate's answers for the vendor's client; classmethods only."""

    # -- errors --------------------------------------------------------------------------------

    @classmethod
    def error(
        cls, message: str, code: str, param: str | None = None, kind: str = "invalid_request_error"
    ) -> dict[str, Any]:
        """An error body in the vendor's shape.

        Args:
            message: What the person reads.
            code: The error's code.
            param: The parameter at fault, if any.
            kind: The error's type.

        Returns:
            dict[str, Any]: `{"error": {"message", "type", "param", "code"}}`.
        """
        return {"error": {"message": message, "type": kind, "param": param, "code": code}}

    @classmethod
    def unknown_url(cls, path: str) -> dict[str, Any]:
        """The vendor's 404 for a path nothing answers (`unknown_url`)."""
        return cls.error(f"Unknown path {path.split('?', 1)[0]}", "unknown_url")

    @classmethod
    def unknown_model(cls, model: str) -> dict[str, Any]:
        """The 404 body for a model nothing serves (`model_not_found`)."""
        return cls.error(
            f"The model `{model}` does not exist or you do not have access to it.",
            "model_not_found",
            "model",
        )

    # -- the model list ------------------------------------------------------------------------

    @classmethod
    def is_codex_query(cls, path: str) -> bool:
        """True when a `/v1/models` request carries Codex's `client_version` query."""
        _, _, query = path.partition("?")
        return any(part.startswith(CODEX_QUERY) for part in query.split("&"))

    @classmethod
    def served_ids(cls, record: object) -> list[str]:
        """The ids in an API-shaped model list, adapters (`<base>+<name>`) included."""
        if not isinstance(record, dict):
            return []
        return [
            str(entry["id"])
            for entry in record.get("data") or []
            if isinstance(entry, dict) and entry.get("id")
        ]

    @classmethod
    def alias_target(cls, model: object, ids: list[str]) -> str | None:
        """The served id an alias stands for; None when `model` is no alias or nothing is served."""
        if isinstance(model, str) and model in ALIASES and model not in ids and ids:
            return ids[0]
        return None

    @classmethod
    def unknown(cls, model: object, ids: list[str]) -> bool:
        """True when a request names a model that is neither served nor an alias."""
        return isinstance(model, str) and bool(ids) and model not in ids and model not in ALIASES

    @classmethod
    def models(cls, record: dict[str, Any], path: str) -> dict[str, Any]:
        """The model list in the shape the request asks for.

        Args:
            record: The API-shaped list (`{"object": "list", "data": [...]}`, adapters applied).
            path: The request's path with its query.

        Returns:
            dict[str, Any]: The API shape with the alias entries added, or `{"models": [...]}`
            (Codex's `ModelInfo` list) when the query has `client_version`.
        """
        data = [entry for entry in record.get("data") or [] if isinstance(entry, dict)]
        served = data[0] if data else {}
        created = served.get("created")
        taken = {str(entry.get("id")) for entry in data}
        aliases = [
            {"id": name, "object": "model", "created": created, "owned_by": ALIAS_OWNER}
            for name in ALIASES
            if name not in taken
        ]
        if not cls.is_codex_query(path):
            return {**record, "object": "list", "data": data + aliases} if data else record
        return {
            "models": [
                cls.model_info(str(entry["id"]), entry, rank)
                for rank, entry in enumerate(data + aliases, start=1)
                if entry.get("id")
            ]
        }

    @classmethod
    def model_info(cls, slug: str, entry: dict[str, Any], priority: int) -> dict[str, Any]:
        """One `ModelInfo` of Codex's list (`protocol/src/openai_models.rs`), from an API entry."""
        window = entry.get("max_model_len")
        info: dict[str, Any] = {
            "slug": slug,
            "display_name": slug,
            "description": f"{slug}, served by this node",
            "supported_reasoning_levels": [],
            "shell_type": "shell_command",
            "visibility": "list",
            "supported_in_api": True,
            "priority": priority,
            "availability_nux": None,
            "upgrade": None,
            "support_verbosity": False,
            "default_verbosity": None,
            "apply_patch_tool_type": "freeform",
            "truncation_policy": {"mode": "tokens", "limit": 10000},
            "experimental_supported_tools": [],
            "used_fallback_model_metadata": False,
            "supports_parallel_tool_calls": True,
        }
        if isinstance(window, int) and window > 0:
            info["context_window"] = window
            info["max_context_window"] = window
        return info

    # -- parameters ----------------------------------------------------------------------------

    @classmethod
    def sanitize(cls, path: str, body: bytes) -> tuple[bytes, str | None]:
        """Removes the parameters the engine rejects, so a client never gets a 400 for them.

        Args:
            path: The request's path.
            body: Its body.

        Returns:
            tuple[bytes, str | None]: The body (the same bytes when nothing was removed) and the
            log note naming what was.
        """
        if path.split("?", 1)[0] not in SANITIZED_PATHS:
            return body, None
        try:
            record = json.loads(body)
        except ValueError:
            return body, None
        if not isinstance(record, dict):
            return body, None
        removed = [name for name in DROPPED_PARAMS if name in record]
        for name in removed:
            del record[name]
        reasoning = record.get("reasoning")
        if isinstance(reasoning, dict):
            for name in DROPPED_REASONING:
                if name in reasoning:
                    del reasoning[name]
                    removed.append(f"reasoning.{name}")
            if not reasoning:
                del record["reasoning"]
        if not removed:
            return body, None
        return json.dumps(record).encode("utf-8"), f"dropped {', '.join(removed)} from {path}"

    # -- limit headers and request ids ---------------------------------------------------------

    @classmethod
    def limit_headers(cls, now: float | None = None) -> list[tuple[str, str]]:
        """The headers Codex's status line reads, saying no limit is reached and credits are unlimited.

        Args:
            now: The time, for the reset stamps (default: now).

        Returns:
            list[tuple[str, str]]: Name and value pairs, including the request ids.
        """
        moment = int(time.time() if now is None else now)
        request_id = str(uuid.uuid4())
        return [
            ("x-codex-primary-used-percent", "0"),
            ("x-codex-primary-window-minutes", str(PRIMARY_WINDOW_MINUTES)),
            (
                "x-codex-primary-reset-at",
                str(moment + PRIMARY_WINDOW_MINUTES * SECONDS_PER_MINUTE),
            ),
            ("x-codex-secondary-used-percent", "0"),
            ("x-codex-secondary-window-minutes", str(SECONDARY_WINDOW_MINUTES)),
            (
                "x-codex-secondary-reset-at",
                str(moment + SECONDARY_WINDOW_MINUTES * SECONDS_PER_MINUTE),
            ),
            ("x-codex-credits-has-credits", "true"),
            ("x-codex-credits-unlimited", "true"),
            ("x-codex-active-limit", "codex"),
            ("x-request-id", request_id),
            ("x-oai-request-id", request_id),
        ]

    @classmethod
    def head_with(cls, head: bytes, extra: list[tuple[str, str]]) -> bytes:
        """A response head with more headers, none of whose names it already has.

        Args:
            head: The head, ending in a blank line.
            extra: Header pairs to add.

        Returns:
            bytes: The head.
        """
        have = {
            line.split(HEADER_SEP, 1)[0].strip().lower()
            for line in head.split(b"\r\n")[1:]
            if HEADER_SEP in line
        }
        add = "".join(
            f"{name}: {value}\r\n" for name, value in extra if name.encode().lower() not in have
        )
        return head[: -len(b"\r\n")] + add.encode("latin-1") + b"\r\n" if add else head

    # -- /backend-api --------------------------------------------------------------------------

    @classmethod
    def is_backend(cls, path: str) -> bool:
        """True for a path under `/backend-api/` (the vendor-only endpoints)."""
        route = path.split("?", 1)[0]
        return route == BACKEND_PREFIX or route.startswith(BACKEND_PREFIX + "/")

    @classmethod
    def user_from(cls, authorization: str | None) -> dict[str, str]:
        """The signed-in user a stub names, read from the Bearer JWT's claims without a check.

        The check is the gate's (§10); a stub only echoes who asked. Anything unreadable names the
        node's local user.

        Args:
            authorization: The `Authorization` header's value.

        Returns:
            dict[str, str]: `id`, `username` and `role`.
        """
        user = {"id": "local", "username": "local", "role": "member"}
        value = (authorization or "").strip()
        if not value.lower().startswith(BEARER):
            return user
        parts = value[len(BEARER) :].split(".")
        if len(parts) != JWT_PARTS:
            return user
        try:
            padded = parts[1] + "=" * (-len(parts[1]) % 4)
            claims = json.loads(base64.urlsafe_b64decode(padded))
        except ValueError:
            return user
        if not isinstance(claims, dict):
            return user
        for field, keys in (
            ("id", ("sub", "user_id")),
            ("username", ("preferred_username", "username", "name")),
            ("role", ("role",)),
        ):
            found = next((str(claims[key]) for key in keys if claims.get(key)), None)
            if found:
                user[field] = found
        return user

    @classmethod
    def backend(cls, path: str, authorization: str | None = None) -> tuple[int, dict[str, Any]]:
        """The gate's answer to a `/backend-api/` request (the table of §5.9).

        Args:
            path: The request's path.
            authorization: The `Authorization` header's value, for the stubs that name the user.

        Returns:
            tuple[int, dict[str, Any]]: The status and the body.
        """
        route = path.split("?", 1)[0].rstrip("/")
        tail = route[len(BACKEND_PREFIX) :].strip("/")
        stub = cls.stub(tail, cls.user_from(authorization))
        if stub is not None:
            return 200, stub
        if tail.startswith(SHAPE_404_TAILS):
            return HTTP_NOT_FOUND, cls.error(f"Not found: {route}", "not_found")
        return HTTP_NOT_FOUND, cls.unknown_url(route)

    @classmethod
    def stub(cls, tail: str, user: dict[str, str]) -> dict[str, Any] | None:
        """The body of the stub for a `/backend-api/` tail; None when no stub answers it."""
        if tail.endswith(USAGE_TAILS):
            return {
                "plan_type": user["role"],
                "rate_limit": {"allowed": True, "limit_reached": False},
                "credits": {"has_credits": True, "unlimited": True, "balance": None},
            }
        if tail.endswith(ACCOUNT_TAILS):
            # The list form with one entry per account: Codex's workspace routing looks the
            # ChatGPT-Account-ID up by `id`, needs the two routing fields (`NO_CONSTRAINT` keeps the
            # gate's own https base URL) and drops a plan_type it does not know, so none is sent.
            return {
                "accounts": [
                    {
                        "id": user["id"],
                        "account_id": user["id"],
                        "name": user["username"],
                        "structure": "personal",
                        "workspace_backend_origin": "NO_CONSTRAINT",
                        "account_routing_override": "NO_CONSTRAINT",
                    }
                ],
                "account_ordering": [user["id"]],
                "default_account_id": user["id"],
            }
        if tail.endswith(PROFILE_TAILS):
            return {
                "id": user["id"],
                "username": user["username"],
                "email": f"{user['username']}@ragon.invalid",
            }
        if tail.endswith(EMPTY_SETTINGS_TAILS):
            return {}
        if tail.endswith(EMPTY_LIST_TAILS):
            return {"items": [], "data": [], "plugins": [], "connectors": []}
        return None

    # -- CORS ----------------------------------------------------------------------------------

    @classmethod
    def origins(cls, state_dir: str) -> list[str]:
        """The origins allowed: the desktop app's and the node's `ragon web` ones.

        Args:
            state_dir: The gate's state folder; `web-origins.json` there lists the web origins
                (`https://<node>.local:3100` and the names the node advertises).

        Returns:
            list[str]: The origins.
        """
        listed: list[str] = []
        try:
            with (Path(state_dir) / ORIGINS_FILE).open(encoding="utf-8") as handle:
                record = json.load(handle)
            if isinstance(record, list):
                listed = [
                    str(item) for item in record if isinstance(item, str) and item != WILDCARD
                ]
        except (OSError, ValueError):
            pass
        return [DESKTOP_ORIGIN, *listed]

    @classmethod
    def cors_headers(
        cls, origin: str | None, allowed: list[str], preflight: bool = False
    ) -> list[tuple[str, str]]:
        """The CORS headers for a request's `Origin`; none unless it is one of `allowed`.

        Args:
            origin: The request's `Origin` header.
            allowed: `origins()`'s answer.
            preflight: True for an `OPTIONS` answer.

        Returns:
            list[tuple[str, str]]: Header pairs, never a wildcard.
        """
        if not origin or origin not in allowed:
            return []
        headers = [("Access-Control-Allow-Origin", origin), ("Vary", "Origin")]
        if preflight:
            headers += [
                ("Access-Control-Allow-Methods", CORS_METHODS),
                ("Access-Control-Allow-Headers", CORS_HEADERS),
                ("Access-Control-Max-Age", CORS_MAX_AGE),
            ]
        return headers
