"""Main Dreamference Configuration Class.

This module provides the DreamferenceConfig class which merges CLI parameters, environment
variables, the configuration file (`ConfigPathResolver`) and system defaults into one object.
"""

import os
import re
import sys
from pathlib import Path
from typing import Any, ClassVar, Final

from dreamference.config.airgapped_resolver import AirgappedResolver
from dreamference.config.config_file_storage_manager import ConfigFileStorageManager
from dreamference.config.config_path_resolver import ConfigPathResolver
from dreamference.config.log_level_spec import LogLevelSpec
from dreamference.config.ports import DEFAULT_VLLM_HOST
from dreamference.hardware import check_model_compatibility
from dreamference.hardware.model_matrix_registry import (
    DEFAULT_DIFFUSION_MODEL_ALIAS,
    DEFAULT_MODEL_ALIAS,
)

# System defaults
DEFAULT_MODEL: Final[str] = DEFAULT_MODEL_ALIAS
DEFAULT_LOG_LEVEL: Final[str] = "info"
DEFAULT_DIFFUSION_MODEL: Final[str] = DEFAULT_DIFFUSION_MODEL_ALIAS
DEFAULT_SPECULATIVE_TOKENS: Final[int] = 8
DEFAULT_AGENT_RUNNER: Final[str] = "codex"
DEFAULT_PREFIX_CACHING: Final[bool] = True
DEFAULT_CHUNKED_PREFILL: Final[bool] = True
DEFAULT_SCHEDULER_STEPS: Final[int] = 8
DEFAULT_ATTENTION_BACKEND: Final[str] = "auto"
# None, not a dtype: this value is handed to the launcher as an explicit caller argument, so any
# concrete default here overrides every model's own kv_cache_dtype recipe. The choice belongs to
# the model — fp8 halves KV but is not accepted by every attention backend — so the config layer
# stays silent unless the operator says otherwise.
DEFAULT_KV_CACHE_DTYPE: Final[str | None] = None
DEFAULT_ENABLE_AUTO_TOOL_CHOICE: Final[bool] = True
DEFAULT_TOOL_CALL_PARSER: Final[str] = "hermes"
DEFAULT_MAX_NUM_BATCHED_TOKENS: Final[int] = 8192

DEFAULT_GUIDED_DECODING_BACKEND: Final[str] = "xgrammar"
# The engine `server start` serves the model with: None is the model's own default (SGLang for the
# default model); `ragon` opts into ragon-engine's ragon-serve, `sglang` names SGLang.
# `server start --engine` overrides it for one start.
DEFAULT_SERVER_ENGINE: Final[str | None] = None
SERVER_ENGINES: Final[tuple] = ("ragon", "sglang", "vllm")
DEFAULT_USE_TENSORIZER: Final[bool] = False
# Whether the Ragon agent's prompt advertises `ragon-admin gmail` when an account is connected.
# Read by the Rust launcher too (DREAMFERENCE_RAGON_GMAIL, then `ragon_gmail` in the TOML file).
DEFAULT_RAGON_GMAIL: Final[bool] = True
# A repository's own file may only turn it off (owner decision); the user's file decides.
GMAIL_KEY: Final[str] = "ragon_gmail"
# Whether the Ragon agent is handed a rule-built ledger (files, failed commands, last test result)
# after each compaction (specs/DREAMFERENCE_RAGON_COMPACTION.md §10.1). Read by the Rust launcher
# (DREAMFERENCE_RAGON_COMPACTION_LEDGER, then `ragon_compaction_ledger` in the TOML file); a test
# keeps this default equal to LEDGER_DEFAULT in ragon-rs/src/compaction.rs.
DEFAULT_RAGON_COMPACTION_LEDGER: Final[bool] = True
# How much of the internet a Ragon session may use (`/airgapped`,
# specs/DREAMFERENCE_RAGON_AIRGAPPED.md): everything or nothing. The Rust
# side (ragon-rs/airgapped) resolves it for the agent's commands; a test keeps this default equal
# to its DEFAULT_RAGON_AIRGAPPED. Listed loosest first.
DEFAULT_RAGON_AIRGAPPED: Final[str] = "off"
RAGON_AIRGAPPED_LEVELS: Final[tuple] = ("off", "on")
# The model server named for this process; trusted over every file.
VLLM_HOST_VARIABLE: Final[str] = "DREAMFERENCE_VLLM_HOST"
# The keys that choose a host, port, URL or other network address. A repository's own
# `dreamference.toml` may set none of them (owner decision): a clone ships that file and
# the agent can write it, so it could send every prompt, and whatever the agent reads, to a machine
# of its choosing. Only the user's own file, the environment and an argument may. A test keeps this
# set equal to NETWORK_KEYS in ragon-rs/src/lib.rs and ragon-code-rs/src/config.rs.
NETWORK_KEYS: Final[frozenset] = frozenset({"vllm_host"})
# The system prompt new ragon sessions start with (`ragon prompt`, specs/DREAMFERENCE_RAGON_PROMPT.md):
# `default` (Codex's own), `high-swe`, or a custom prompt in `$CODEX_HOME/system-prompts/<name>.md`.
# The Rust launcher reads it too (DREAMFERENCE_RAGON_PROMPT, then `ragon_prompt` in the TOML file)
# and decides whether a name is installed; a test keeps this default equal to DEFAULT_PROMPT in
# ragon-rs/src/prompt.rs.
DEFAULT_RAGON_PROMPT: Final[str] = "default"
RAGON_PROMPT_NAME: Final[re.Pattern] = re.compile(r"[a-z0-9][a-z0-9-]{0,63}")
# Refine mode (specs/DREAMFERENCE_RAGON_REFINE.md): each new task is first studied by a session
# that changes nothing and writes a refined description, then done by a fresh session given the
# task and that description, in `ragon exec`, the interactive session and Night Shift alike. Off
# until the 100-task SWE-bench pair decides (spec §3); switching it on is this line and DEFAULT in
# ragon-rs/src/refine.rs, which a test keeps equal. The launcher reads the same tiers
# (DREAMFERENCE_RAGON_REFINE, then `ragon_refine` in the TOML file), after its `--refine` flag.
DEFAULT_RAGON_REFINE: Final[bool] = False
# Which texts refine mode uses (spec §10): `v1`, the measured ones, or `v2`. Through the same tiers
# (DREAMFERENCE_RAGON_REFINE_VERSION, then `ragon_refine_version`), after the launcher's
# `--refine-version`; a value that is not a version passes to the next tier. A test keeps this
# equal to DEFAULT_VERSION in ragon-rs/src/refine.rs.
DEFAULT_RAGON_REFINE_VERSION: Final[str] = "v1"
RAGON_REFINE_VERSIONS: Final[tuple] = ("v1", "v2")

# The keys save_config() writes when they differ from their default and removes when they do not;
# every other key in the file belongs to another reader and is kept.
CONFIG_KEYS: Final[frozenset] = frozenset(
    {
        "vllm_host",
        "agent_runner",
        "model",
        "diffusion_model",
        "draft_model",
        "num_speculative_tokens",
        "hf_token",
        "enable_prefix_caching",
        "enable_chunked_prefill",
        "num_scheduler_steps",
        "attention_backend",
        "kv_cache_dtype",
        "guided_decoding_backend",
        "use_tensorizer",
        "server_engine",
        "ragon_gmail",
        "ragon_compaction_ledger",
        "ragon_airgapped",
        "ragon_prompt",
        "ragon_refine",
        "ragon_refine_version",
    }
)
# Keys this release does not read: removed on save like a default.
RETIRED_KEYS: Final[frozenset] = frozenset({"cave_mode", "ragon_cave_mode"})


class DreamferenceConfig:
    """Unified configuration manager implementing the 4-tier precedence hierarchy.

    CLI Parameters > Environment Variables > the configuration file (`dreamference.toml`, …) >
    System Defaults.
    """

    # (file, key) pairs already reported as ignored in this process, so each is said once.
    _warned: ClassVar[set[tuple[Path, str]]] = set()

    def __init__(
        self,
        config_file: str | None = None,
        vllm_host: str | None = None,
        model: str | None = None,
        diffusion_model: str | None = None,
        draft_model: str | None = None,
        num_speculative_tokens: int | None = None,
        agent_runner: str | None = None,
        hf_token: str | None = None,
        enable_prefix_caching: bool | None = None,
        enable_chunked_prefill: bool | None = None,
        num_scheduler_steps: int | None = None,
        attention_backend: str | None = None,
        kv_cache_dtype: str | None = None,
        use_tensorizer: bool | None = None,
        guided_decoding_backend: str | None = None,
        server_engine: str | None = None,
        ragon_gmail: bool | None = None,
        ragon_compaction_ledger: bool | None = None,
        ragon_airgapped: str | None = None,
        ragon_prompt: str | None = None,
        ragon_refine: bool | None = None,
        ragon_refine_version: str | None = None,
        log_level: str | None = None,
    ):
        """Initializes DreamferenceConfig by loading file defaults and overriding with environment variables and parameters."""
        self.config_file_path: Path = ConfigPathResolver.resolve_path(config_file)
        self.file_data: dict[str, Any] = ConfigFileStorageManager.load_config_dict(
            self.config_file_path
        )
        # A repository's network keys, ignored and kept aside so save_config() leaves them as they were.
        self._repository_network: dict[str, Any] = {}
        self._repository_file: bool = ConfigPathResolver.is_repository_file(
            self.config_file_path, config_file
        )
        if self._repository_file:
            self._take_network_keys_from_user_file()
            self._let_repository_turn_gmail_only_off()

        # The values another source supplied (the environment, or the user's own file in place of
        # a repository's host), which save_config() leaves as the file held them.
        self._from_environment: dict[str, Any] = {}

        # 1. Target vLLM server endpoint host URL
        if vllm_host is not None:
            self.vllm_host: str = vllm_host
        elif VLLM_HOST_VARIABLE in os.environ:
            self.vllm_host = os.environ[VLLM_HOST_VARIABLE]
        else:
            self.vllm_host = str(self.file_data.get("vllm_host", DEFAULT_VLLM_HOST))

        # 2. Main target LLM model name served on vLLM.
        #
        # `_model_pinned` records whether this value was *chosen* or merely defaulted to, which is
        # not the same question as whether it currently differs from the default. save_config()
        # needs the distinction: a model that happens to equal today's DEFAULT_MODEL is still an
        # explicit choice, and dropping it from the file on that basis silently re-points the
        # workspace the next time the default moves. Anything that arrived from a constructor
        # argument, the environment, or the config file counts as chosen; only falling all the way
        # through to DEFAULT_MODEL does not.
        self._model_pinned: bool = (
            model is not None
            or os.getenv("DREAMFERENCE_MODEL") is not None
            or "model" in self.file_data
        )
        # Assigned to the backing field rather than through the property: resolution is not a
        # choice, and going through the setter here would mark every config as pinned.
        self._model: str = (
            model
            if model is not None
            else os.getenv("DREAMFERENCE_MODEL", str(self.file_data.get("model", DEFAULT_MODEL)))
        )

        # 2b. Diffusion model served in parallel with the main model. Every configuration names
        # one, exactly as it names a main model, and the pinning rules are the same for the same
        # reason: a diffusion model that happens to equal today's default is still a choice, and
        # dropping it from the file re-points the workspace when the default moves.
        self._diffusion_model_pinned: bool = (
            diffusion_model is not None
            or os.getenv("DREAMFERENCE_DIFFUSION_MODEL") is not None
            or "diffusion_model" in self.file_data
        )
        self._diffusion_model: str = (
            diffusion_model
            if diffusion_model is not None
            else os.getenv(
                "DREAMFERENCE_DIFFUSION_MODEL",
                str(self.file_data.get("diffusion_model", DEFAULT_DIFFUSION_MODEL)),
            )
        )

        # 3. Optional speculative decoding draft model name
        self.draft_model: str | None = (
            draft_model
            if draft_model is not None
            else os.getenv("DREAMFERENCE_DRAFT_MODEL", self.file_data.get("draft_model", None))
        )

        # 4. Number of speculative draft tokens to propose per step
        env_spec_tokens = os.getenv("DREAMFERENCE_SPECULATIVE_TOKENS")
        if num_speculative_tokens is not None:
            self.num_speculative_tokens: int = num_speculative_tokens
        elif env_spec_tokens is not None:
            self.num_speculative_tokens = int(env_spec_tokens)
        else:
            self.num_speculative_tokens = int(
                self.file_data.get("num_speculative_tokens", DEFAULT_SPECULATIVE_TOKENS)
            )

        # 6. Agent runner ('codex' [default], 'cline', 'continue' or 'openhands')
        self.agent_runner: str = (
            agent_runner
            if agent_runner is not None
            else os.getenv(
                "DREAMFERENCE_AGENT",
                os.getenv(
                    "DREAMFERENCE_RUNNER",
                    str(self.file_data.get("agent_runner", DEFAULT_AGENT_RUNNER)),
                ),
            )
        ).lower()

        # 7. HuggingFace access token. Whether save_config() may write it is decided by where it
        # came from: a token from the environment stays there, because the file is by default
        # `dreamference.toml` in the working directory, a repository the agent reads and a commit
        # can carry away. One named as an argument is written, and one the file already holds is
        # kept as it was, even while the environment overrides it.
        self._hf_token_to_save: str | None = (
            hf_token if hf_token is not None else self.file_data.get("hf_token")
        )
        self.hf_token: str | None = (
            hf_token
            if hf_token is not None
            else os.getenv(
                "HF_TOKEN", os.getenv("DREAMFERENCE_HF_TOKEN", self.file_data.get("hf_token", None))
            )
        )

        # 8. Blackwell GB10 Performance Tuning Flags
        self.enable_prefix_caching: bool = bool(
            enable_prefix_caching
            if enable_prefix_caching is not None
            else self.file_data.get("enable_prefix_caching", DEFAULT_PREFIX_CACHING)
        )

        self.enable_chunked_prefill: bool = bool(
            enable_chunked_prefill
            if enable_chunked_prefill is not None
            else self.file_data.get("enable_chunked_prefill", DEFAULT_CHUNKED_PREFILL)
        )

        self.num_scheduler_steps: int = int(
            num_scheduler_steps
            if num_scheduler_steps is not None
            else self.file_data.get("num_scheduler_steps", DEFAULT_SCHEDULER_STEPS)
        )

        self.attention_backend: str = str(
            attention_backend
            if attention_backend is not None
            else self.file_data.get("attention_backend", DEFAULT_ATTENTION_BACKEND)
        )

        # None means "unset — whatever the model's recipe asks for", and the str() coercion that
        # used to wrap this had to go with it, since it turned None into the string "None".
        #
        # This field is passed straight into start_server, where a non-None value counts as an
        # explicit caller choice and outranks the registry recipe. Defaulting it to a concrete
        # 'fp8' therefore made every model's kv_cache_dtype recipe entry unreachable through the
        # CLI — silently, because the two NVFP4 recipes ask for fp8 anyway. The first model that
        # wanted something else got fp8 regardless and failed to load: FlashAttention rejects an
        # fp8 KV cache outright ("Selected backend FLASH_ATTN is not valid for this configuration.
        # Reason: ['kv_cache_dtype not supported']").
        self.kv_cache_dtype: str | None = (
            kv_cache_dtype
            if kv_cache_dtype is not None
            else self.file_data.get("kv_cache_dtype", DEFAULT_KV_CACHE_DTYPE)
        )

        self.guided_decoding_backend: str = str(
            guided_decoding_backend
            if guided_decoding_backend is not None
            else self.file_data.get("guided_decoding_backend", DEFAULT_GUIDED_DECODING_BACKEND)
        )

        # File only, and None when the file is silent: `server start` then takes the model
        # recipe's value, else hermes and 8192. A default here would override every recipe.
        file_parser = self.file_data.get("tool_call_parser")
        self.tool_call_parser: str | None = str(file_parser) if file_parser else None
        file_batched = self.file_data.get("max_num_batched_tokens")
        self.max_num_batched_tokens: int | None = (
            int(file_batched) if file_batched is not None else None
        )

        # The model server's engine, through the same tiers; an unknown name is skipped, so a typo
        # falls through to the model's default instead of stopping `server start`.
        self.server_engine: str | None = DEFAULT_SERVER_ENGINE
        for candidate in (
            server_engine,
            os.getenv("DREAMFERENCE_SERVER_ENGINE"),
            self.file_data.get("server_engine"),
        ):
            if isinstance(candidate, str) and candidate.strip().lower() in SERVER_ENGINES:
                self.server_engine = candidate.strip().lower()
                break

        env_tensorizer = os.getenv("DREAMFERENCE_USE_TENSORIZER")
        if use_tensorizer is not None:
            self.use_tensorizer: bool = use_tensorizer
        elif env_tensorizer:  # empty is unset, as for the other switches
            self.use_tensorizer = env_tensorizer.lower() in ("1", "true", "yes")
        else:
            self.use_tensorizer = bool(self.file_data.get("use_tensorizer", DEFAULT_USE_TENSORIZER))

        env_ragon_gmail = os.getenv("DREAMFERENCE_RAGON_GMAIL")
        if ragon_gmail is not None:
            self.ragon_gmail: bool = ragon_gmail
        elif env_ragon_gmail:  # empty is unset, as in the Rust launcher
            self.ragon_gmail = env_ragon_gmail.lower() in ("1", "true", "yes")
        else:
            self.ragon_gmail = bool(self.file_data.get("ragon_gmail", DEFAULT_RAGON_GMAIL))

        env_ledger = os.getenv("DREAMFERENCE_RAGON_COMPACTION_LEDGER")
        if ragon_compaction_ledger is not None:
            self.ragon_compaction_ledger: bool = ragon_compaction_ledger
        elif env_ledger:
            self.ragon_compaction_ledger = env_ledger.lower() in ("1", "true", "yes", "on")
        else:
            self.ragon_compaction_ledger = bool(
                self.file_data.get("ragon_compaction_ledger", DEFAULT_RAGON_COMPACTION_LEDGER)
            )

        # The air-gap level, through the same tiers; an invalid value is skipped. This reads the one
        # configuration file this object resolved: the launcher also reads the user-level file and
        # takes the stricter of the two, because the agent can write the repository's.
        self.ragon_airgapped: str = DEFAULT_RAGON_AIRGAPPED
        for tier, candidate in enumerate(
            (
                ragon_airgapped,
                os.getenv("DREAMFERENCE_RAGON_AIRGAPPED"),
                self.file_data.get("ragon_airgapped"),
            )
        ):
            level = self.parse_airgapped_level(candidate)
            if level is not None:
                self.ragon_airgapped = level
                if tier == 1:
                    self._from_environment["ragon_airgapped"] = level
                break

        # The log level spec, through the same tiers (LOGGING §4.3): a blank value is unset, and
        # LogLevelSpec ignores the spec's unknown parts when it is read, not here.
        self.log_level: str = DEFAULT_LOG_LEVEL
        for candidate in (
            log_level,
            os.getenv("RAGON_LOG"),
            self.file_data.get("log_level"),
        ):
            if LogLevelSpec.is_set(candidate):
                self.log_level = str(candidate).strip()
                break

        # The prompt's name, through the same tiers. Only its form is checked here: which prompts
        # are installed is known to the launcher, which skips a name it does not have.
        self.ragon_prompt: str = DEFAULT_RAGON_PROMPT
        for candidate in (
            ragon_prompt,
            os.getenv("DREAMFERENCE_RAGON_PROMPT"),
            self.file_data.get("ragon_prompt"),
        ):
            name = self.parse_prompt_name(candidate)
            if name is not None:
                self.ragon_prompt = name
                break

        # Refine mode, through the same tiers and the same reading of a value as the launcher's:
        # an empty (or blank) variable counts as unset, anything else is on only if it says so.
        env_refine = (os.getenv("DREAMFERENCE_RAGON_REFINE") or "").strip()
        if ragon_refine is not None:
            self.ragon_refine: bool = ragon_refine
        elif env_refine:
            self.ragon_refine = env_refine.lower() in ("1", "true", "yes", "on")
        else:
            file_refine = self.file_data.get("ragon_refine", DEFAULT_RAGON_REFINE)
            self.ragon_refine = (
                file_refine if isinstance(file_refine, bool) else DEFAULT_RAGON_REFINE
            )
        self.ragon_refine_version: str = DEFAULT_RAGON_REFINE_VERSION
        for candidate in (
            ragon_refine_version,
            os.getenv("DREAMFERENCE_RAGON_REFINE_VERSION"),
            self.file_data.get("ragon_refine_version"),
        ):
            version = candidate.strip().lower() if isinstance(candidate, str) else None
            if version in RAGON_REFINE_VERSIONS:
                self.ragon_refine_version = version
                break

    @property
    def model(self) -> str:
        """The main target LLM served on vLLM.

        Returns:
            str: Model alias, HuggingFace repo ID, or display name.
        """
        return self._model

    @model.setter
    def model(self, value: str) -> None:
        """Sets the model and records that it was chosen deliberately.

        Assigning a model is what `ragon-admin main-model set` does, and it is a pin by definition — the
        caller named this model. That has to be remembered separately from the value itself, or
        save_config() cannot tell a deliberate choice from a value that merely matches today's
        default, and would drop the former on the floor.

        Args:
            value (str): Model alias, HuggingFace repo ID, or display name.
        """
        self._model = value
        self._model_pinned = True

    @property
    def diffusion_model(self) -> str:
        """The diffusion model served beside the main one.

        Returns:
            str: Model alias, HuggingFace repo ID, or display name.
        """
        return self._diffusion_model

    @diffusion_model.setter
    def diffusion_model(self, value: str) -> None:
        """Sets the diffusion model and records that it was chosen deliberately.

        Assigning through this property is what `ragon-admin diffusion-model set` does, and it is a pin
        by definition -- same contract as the main model's setter above.

        Args:
            value (str): Model alias, HuggingFace repo ID, or display name.
        """
        self._diffusion_model = value
        self._diffusion_model_pinned = True

    @classmethod
    def parse_airgapped_level(cls, value: Any) -> str | None:
        """Reads an air-gap level as the launcher does: a level name in any case.

        `duckduckgo` was a level until recently and is now an unknown name, like any other.

        A YAML file turns a bare `on` or `off` into a boolean, so those are accepted too.

        Args:
            value: What a tier holds.

        Returns:
            Optional[str]: `off` or `on`; None for anything else.
        """
        if isinstance(value, bool):
            return "on" if value else "off"
        if not isinstance(value, str):
            return None
        name = value.strip().lower()
        return name if name in RAGON_AIRGAPPED_LEVELS else None

    @classmethod
    def parse_prompt_name(cls, value: Any) -> str | None:
        """Reads a prompt's name as the launcher does: lowercase letters, digits and hyphens.

        Args:
            value: What a tier holds.

        Returns:
            Optional[str]: The name, stripped; None when it is not one a prompt can have.
        """
        if not isinstance(value, str):
            return None
        name = value.strip()
        return name if RAGON_PROMPT_NAME.fullmatch(name) else None

    @classmethod
    def resolve_airgapped_level(cls, cwd: Path | None = None) -> str:
        """Resolves the configured air-gap level for a command with no session.

        The one reader decides (specs/DREAMFERENCE_RAGON_AIRGAPPED.md §6.5): `ragon airgapped
        --json`, through `AirgappedResolver`, which takes `on` when `ragon` cannot answer. Its tiers
        are `DREAMFERENCE_RAGON_AIRGAPPED`, then the **stricter** of the file
        `DREAMFERENCE_CONFIG_PATH` names (or `<cwd>/dreamference.toml`) and the user-level
        `~/.config/dreamference/config.toml`, then the default; a file it cannot read counts as
        `on`. An instance's `ragon_airgapped` reads one file only, so anything that *acts* on
        the level (the MCP server's web tools) asks here.

        Args:
            cwd: The directory whose `dreamference.toml` counts; defaults to the current one.

        Returns:
            str: `off` or `on`.
        """
        return AirgappedResolver.level(cwd=cwd)

    def _take_network_keys_from_user_file(self) -> None:
        """Drop the repository's network keys from `file_data`, saying so, and take the user's own.

        The repository's file shadows the user's whole, so without the user's values here a key the
        repository may not set would fall to its default even when the user named one.
        """
        user_data = ConfigFileStorageManager.load_config_dict(ConfigPathResolver.user_path())
        for key in sorted(NETWORK_KEYS):
            if key in self.file_data:
                self._repository_network[key] = self.file_data.pop(key)
                self._warn_ignored(key)
            if key in user_data:
                self.file_data[key] = user_data[key]

    def _let_repository_turn_gmail_only_off(self) -> None:
        """Keep a repository's `ragon_gmail = false` and ignore its `true` (owner decision).

        A clone ships this file and the agent can write it, so it must not reopen Gmail after the
        user closed it. The user's own file decides, unless the repository's closes it.
        """
        if self.file_data.get(GMAIL_KEY) is False:
            return
        self.file_data.pop(GMAIL_KEY, None)
        user_data = ConfigFileStorageManager.load_config_dict(ConfigPathResolver.user_path())
        if GMAIL_KEY in user_data:
            self.file_data[GMAIL_KEY] = user_data[GMAIL_KEY]

    def _warn_ignored(self, key: str) -> None:
        """Say once per process, file and key that a repository's network key was not used.

        Args:
            key (str): The key the repository's file set.
        """
        notice = (self.config_file_path.resolve(), key)
        if notice in DreamferenceConfig._warned:
            return
        DreamferenceConfig._warned.add(notice)
        # The value is left out: it is the repository's, and could hold terminal escapes.
        sys.stderr.write(
            f"Ignored {key} in {self.config_file_path}: a repository's own file may not choose a network "
            f"address. Set it in ~/.config/dreamference/config.toml or {VLLM_HOST_VARIABLE} instead.\n"
        )

    def _log_level_entry(self) -> dict[str, Any]:
        """Return the `log_level` entry for the config file, or nothing when it is the default.

        Returns:
            dict[str, Any]: `{"log_level": spec}` when the spec differs from DEFAULT_LOG_LEVEL.
        """
        return {"log_level": self.log_level} if self.log_level != DEFAULT_LOG_LEVEL else {}

    def save_config(self, target_path: Path | None = None) -> Path:
        """Saves current active configuration parameters to the TOML, YAML or JSON config file.

        Args:
            target_path (Optional[Path]): Target output file path. Defaults to self.config_file_path.

        Returns:
            Path: Written config file path.
        """
        out_path = target_path or self.config_file_path
        data: dict[str, Any] = self._log_level_entry()
        # Originally these two were written whatever their value, the only fields that broke
        # the rule the rest of this method follows: a default written to the file stops following
        # the default, so a later change of DEFAULT_VLLM_HOST would have skipped every saved config.
        if self.vllm_host != DEFAULT_VLLM_HOST:
            data["vllm_host"] = self.vllm_host
        if self.agent_runner != DEFAULT_AGENT_RUNNER:
            data["agent_runner"] = self.agent_runner
        # Written whenever the model was chosen, not merely whenever it differs from the default.
        # The two came apart the moment DEFAULT_MODEL changed: a workspace pinned to what was then
        # the default had written no `model` key at all, so it silently followed the default to a
        # different checkpoint. Every other field below can be re-derived from its default; this
        # one carries intent.
        if self.model and (self._model_pinned or self.model != DEFAULT_MODEL):
            data["model"] = self.model
        # Same intent-carrying rule as `model` above, for the same reason.
        if self.diffusion_model and (
            self._diffusion_model_pinned or self.diffusion_model != DEFAULT_DIFFUSION_MODEL
        ):
            data["diffusion_model"] = self.diffusion_model
        if self.draft_model is not None:
            data["draft_model"] = self.draft_model
        if self.num_speculative_tokens != DEFAULT_SPECULATIVE_TOKENS:
            data["num_speculative_tokens"] = self.num_speculative_tokens
        if self._hf_token_to_save is not None:
            data["hf_token"] = self._hf_token_to_save
        if self.enable_prefix_caching != DEFAULT_PREFIX_CACHING:
            data["enable_prefix_caching"] = self.enable_prefix_caching
        if self.enable_chunked_prefill != DEFAULT_CHUNKED_PREFILL:
            data["enable_chunked_prefill"] = self.enable_chunked_prefill
        if self.num_scheduler_steps != DEFAULT_SCHEDULER_STEPS:
            data["num_scheduler_steps"] = self.num_scheduler_steps
        if self.attention_backend != DEFAULT_ATTENTION_BACKEND:
            data["attention_backend"] = self.attention_backend
        if self.kv_cache_dtype != DEFAULT_KV_CACHE_DTYPE:
            data["kv_cache_dtype"] = self.kv_cache_dtype
        if self.guided_decoding_backend != DEFAULT_GUIDED_DECODING_BACKEND:
            data["guided_decoding_backend"] = self.guided_decoding_backend
        if self.use_tensorizer != DEFAULT_USE_TENSORIZER:
            data["use_tensorizer"] = self.use_tensorizer
        if self.server_engine != DEFAULT_SERVER_ENGINE:
            data["server_engine"] = self.server_engine
        if self.ragon_gmail != DEFAULT_RAGON_GMAIL:
            data["ragon_gmail"] = self.ragon_gmail
        if self.ragon_compaction_ledger != DEFAULT_RAGON_COMPACTION_LEDGER:
            data["ragon_compaction_ledger"] = self.ragon_compaction_ledger
        if self.ragon_airgapped != DEFAULT_RAGON_AIRGAPPED:
            data["ragon_airgapped"] = self.ragon_airgapped
        if self.ragon_prompt != DEFAULT_RAGON_PROMPT:
            data["ragon_prompt"] = self.ragon_prompt
        if self.ragon_refine != DEFAULT_RAGON_REFINE:
            data["ragon_refine"] = self.ragon_refine
        if self.ragon_refine_version != DEFAULT_RAGON_REFINE_VERSION:
            data["ragon_refine_version"] = self.ragon_refine_version

        # A value still as the environment gave it stays what the file held, or absent.
        for key, value in self._from_environment.items():
            if getattr(self, key) == value:
                data.pop(key, None)
                if key in self.file_data:
                    data[key] = self.file_data[key]

        self._keep_network_keys(data, out_path)
        return ConfigFileStorageManager.save_config_dict(
            out_path, data, owned=CONFIG_KEYS | RETIRED_KEYS
        )

    def _keep_network_keys(self, data: dict[str, Any], out_path: Path) -> None:
        """Leaves a repository's file's network keys as they were, and gives it none.

        It may not choose an address, and one written there would only be ignored. Any other file
        is left to save_config().

        Args:
            data: What save_config() is about to write, changed in place.
            out_path: The file save_config() writes.
        """
        if not self._repository_file or out_path != self.config_file_path:
            return
        for key in sorted(NETWORK_KEYS):
            value = data.pop(key, None)
            if key in self._repository_network:
                data[key] = self._repository_network[key]
            elif value != self.file_data.get(key):
                sys.stderr.write(
                    f"⚠️  {key} not saved in {out_path}: a repository's own file may not choose a network "
                    "address, so a change (or a reset to the default) is not kept while it is in use. "
                    "Set it in ~/.config/dreamference/config.toml.\n"
                )

    def resolve_tool_call_parser(self) -> str:
        """Returns the vLLM tool-call parser that matches the configured model.

        Returns:
            str: Parser name (e.g. 'hermes', 'qwen3_xml').
        """
        from dreamference.vllm_server.vllm_server_manager import VLLMServerManager

        return VLLMServerManager().resolve_tool_call_parser(self.model)

    def validate_model(self) -> tuple[bool, str]:
        """Validates selected main and draft models against GB10 hardware memory specs.

        Returns:
            Tuple[bool, str]: Tuple of (is_valid, validation_message).
        """
        if self.draft_model:
            from dreamference.hardware import check_speculative_compatibility

            return check_speculative_compatibility(self.model, self.draft_model)
        return check_model_compatibility(self.model)
