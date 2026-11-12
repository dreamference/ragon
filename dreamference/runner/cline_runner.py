"""Cline VS Code Session Runner & Configurator for Dreamference.

This module provides the ClineRunner class which configures workspace `.clinerules`,
ensures local vLLM server readiness, provisions the Cline extension, and launches VS Code.
"""

import subprocess
import sys
from pathlib import Path

from dreamference.config import DreamferenceConfig
from dreamference.runner.cline_installer import ClineInstaller
from dreamference.runner.vllm_readiness_waiter import VLLMReadinessWaiter
from dreamference.vllm_server import VLLMServerManager


def _say(text: str) -> None:
    """Writes one line for the person launching Cline."""
    sys.stdout.write(text + "\n")


class ClineRunner:
    """Runner class orchestrating Cline VS Code agent sessions on local GB10 vLLM endpoints."""

    def __init__(self, config: DreamferenceConfig | None = None):
        """Initializes ClineRunner with configuration and vLLM server manager instances.

        Args:
            config (Optional[DreamferenceConfig]): Configuration instance (defaults to DreamferenceConfig()).
        """
        self.config: DreamferenceConfig = config or DreamferenceConfig()
        self.vllm_manager: VLLMServerManager = VLLMServerManager(host=self.config.vllm_host)

    def ensure_clinerules(self) -> Path:
        """Creates or updates workspace `.clinerules` file to configure GB10 vLLM local backend rules.

        Returns:
            Path: Path to written `.clinerules` file.
        """
        rules_path = Path.cwd() / ".clinerules"
        hf_model = VLLMReadinessWaiter.served_model(self.vllm_manager, self.config.model)

        content = (
            "# Ragon Local GB10 AI Agent Rules for Cline\n\n"
            "## Model & Endpoint Configuration\n"
            f"- API Provider: OpenAI Compatible\n"
            f"- Base URL: {self.config.vllm_host}/v1\n"
            f"- Model ID: {hf_model}\n"
            "- Target Hardware: NVIDIA GB10 Blackwell (128GB Unified Memory)\n\n"
            "## Autonomous Execution Guidelines\n"
            "- Always execute linter diagnostics and test suites after modifying code.\n"
            "- Leverage local vLLM prefix caching by reusing system prompt context.\n"
            "- Operate completely offline with zero data egress.\n"
        )

        if not rules_path.exists():
            with Path(rules_path).open("w", encoding="utf-8") as f:
                f.write(content)

        return rules_path

    def run_session(self, prompt: str | None = None, debug: bool = False) -> int:
        """Configures the workspace, installs the Cline extension if missing and launches VS Code.

        It checks the model server health first.

        Args:
            prompt (Optional[str]): Task prompt text.
            debug (bool): Enable debug output.

        Returns:
            int: Subprocess exit code (0 for success).
        """
        # Step 1: Ensure vLLM local server is online
        waiter = VLLMReadinessWaiter(config=self.config)
        if not self.vllm_manager.check_health() and not waiter.wait_for_vllm():
            return 1

        # Step 2: Ensure VS Code and Cline extension are installed
        if not ClineInstaller.is_vscode_installed():
            _say("❌ VS Code CLI (`code` or `codium`) is not installed or not in PATH.")
            _say("💡 Install VS Code or add `code` to PATH to launch Cline.")
            return 1

        ClineInstaller.install_cline_if_missing()

        # Step 3: Write .clinerules in workspace
        rules_file = self.ensure_clinerules()
        _say(f"✅ Configured Cline rules at {rules_file}")

        # Step 4: Display connection details for user
        hf_model = VLLMReadinessWaiter.served_model(self.vllm_manager, self.config.model)
        _say("\n[Cline Configuration Details]")
        _say("  API Provider: OpenAI Compatible")
        _say(f"  Base URL:     {self.config.vllm_host}/v1")
        _say(f"  Model ID:     {hf_model}")
        _say("  API Key:      gb10-local-token")
        if prompt:
            _say(f"  Prompt:       {prompt}")

        # Step 5: Launch VS Code in workspace directory
        code_bin = ClineInstaller.get_vscode_executable()
        _say(f"\n🚀 Launching VS Code with Cline extension for {self.config.model}...")
        try:
            if code_bin is None:
                raise FileNotFoundError("no `code` or `codium` found")
            return subprocess.call([code_bin, str(Path.cwd())])
        except OSError as e:
            _say(f"❌ Failed to launch VS Code: {e}")
            return 1
