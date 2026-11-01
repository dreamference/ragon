"""Hardware: the model matrix, downloads and tensorization, GB10 telemetry and LoRA adapters."""

from pathlib import Path
from typing import Any

from dreamference.hardware.adapter_base_digest import AdapterBaseDigest
from dreamference.hardware.adapter_catalog import AdapterCatalog
from dreamference.hardware.adapter_installer import AdapterInstaller
from dreamference.hardware.adapter_manifest import AdapterManifest
from dreamference.hardware.adapter_signature import AdapterSignature
from dreamference.hardware.adapter_store import AdapterStore
from dreamference.hardware.adapter_verifier import AdapterVerifier
from dreamference.hardware.hardware_manager import HardwareManager
from dreamference.hardware.hardware_telemetry import HardwareTelemetry
from dreamference.hardware.memory_metrics import MemoryMetrics
from dreamference.hardware.model_downloader import ModelDownloader
from dreamference.hardware.model_matrix_registry import ModelMatrixRegistry
from dreamference.hardware.model_spec import ModelSpec
from dreamference.hardware.release_asset_downloader import ReleaseAssetDownloader

MODEL_MATRIX = ModelMatrixRegistry.MATRIX


def resolve_model_hf_repo(model_key: str) -> str:
    """Delegates to `ModelMatrixRegistry.resolve_hf_repo`."""
    return ModelMatrixRegistry.resolve_hf_repo(model_key)


def model_key_for_served_id(served_id: str, preferred: str | None = None) -> str | None:
    """Delegates to `ModelMatrixRegistry.key_for_served_id`."""
    return ModelMatrixRegistry.key_for_served_id(served_id, preferred)


def get_model_launch_overrides(model_key: str, engine: str | None = None) -> dict[str, Any]:
    """The model's launch recipe for an engine (ModelMatrixRegistry.get_launch_overrides)."""
    return ModelMatrixRegistry.get_launch_overrides(model_key, engine)


def get_speculative_draft_repo(model_key: str) -> str | None:
    """Delegates to `ModelMatrixRegistry.get_speculative_draft_repo`."""
    return ModelMatrixRegistry.get_speculative_draft_repo(model_key)


def model_supports_vision(model_key: str) -> bool:
    """Delegates to `ModelMatrixRegistry.supports_vision`."""
    return ModelMatrixRegistry.supports_vision(model_key)


def model_is_diffusion(model_key: str) -> bool:
    """Delegates to `ModelMatrixRegistry.is_diffusion`."""
    return ModelMatrixRegistry.is_diffusion(model_key)


def model_declares_own_quantization(model_key: str) -> bool:
    """Delegates to `ModelMatrixRegistry.declares_own_quantization`."""
    return ModelMatrixRegistry.declares_own_quantization(model_key)


def is_model_downloaded(model_key: str) -> bool:
    """Delegates to `ModelDownloader.is_model_downloaded`."""
    return ModelDownloader.is_model_downloaded(model_key)


def is_model_tensorized(model_key: str) -> bool:
    """Delegates to `ModelDownloader.is_model_tensorized`."""
    return ModelDownloader.is_model_tensorized(model_key)


def tensorize_model(model_key: str, force: bool = False, hf_token: str | None = None) -> bool:
    """Delegates to `ModelDownloader.tensorize_model`."""
    return ModelDownloader.tensorize_model(model_key, force=force, hf_token=hf_token)


def get_tensorized_path(model_key: str) -> Path | None:
    """Delegates to `ModelDownloader.get_tensorized_path`."""
    return ModelDownloader.get_tensorized_path(model_key)


def download_model(
    model_key: str,
    hf_token: str | None = None,
    auto_tensorize: bool = False,
    revision: str | None = None,
) -> bool:
    """Delegates to `ModelDownloader.download_model`."""
    return ModelDownloader.download_model(
        model_key, hf_token=hf_token, auto_tensorize=auto_tensorize, revision=revision
    )


def download_model_with_drafter(
    model_key: str, hf_token: str | None = None, auto_tensorize: bool = False
) -> bool:
    """Fetches a model and its recipe's drafter, each at its pinned commit (ModelDownloader.download_with_drafter)."""
    return ModelDownloader.download_with_drafter(
        model_key, hf_token=hf_token, auto_tensorize=auto_tensorize
    )


def download_recipe_companions(
    model_key: str, hf_token: str | None = None, engine: str | None = None
) -> bool:
    """Fetches the recipe's drafter and engine assets for the served engine (ModelDownloader.download_companions)."""
    return ModelDownloader.download_companions(model_key, hf_token=hf_token, engine=engine)


def download_all_models(
    hf_token: str | None = None, auto_tensorize: bool = False
) -> dict[str, bool]:
    """Delegates to `ModelDownloader.download_all_models`."""
    return ModelDownloader.download_all_models(hf_token=hf_token, auto_tensorize=auto_tensorize)


def clear_model_cache() -> bool:
    """Delegates to `ModelDownloader.clear_cache`."""
    return ModelDownloader.clear_cache()


def clear_tensorizer_cache() -> bool:
    """Delegates to `ModelDownloader.clear_tensorizer_cache`."""
    return ModelDownloader.clear_tensorizer_cache()


def get_system_memory() -> dict[str, float]:
    """Return the host's memory as plain numbers in GB."""
    m = HardwareManager.get_system_memory()
    return {"total_gb": m.total_gb, "available_gb": m.available_gb, "used_gb": m.used_gb}


def detect_gb10_hardware() -> dict[str, Any]:
    """Detect a GB10 and return what was found as a dict."""
    hw = HardwareManager.detect_gb10_hardware()
    return {
        "is_gb10": hw.is_gb10,
        "gpu_name": hw.gpu_name,
        "driver_version": hw.driver_version,
        "total_unified_memory_gb": hw.total_unified_memory_gb,
        "available_memory_gb": hw.available_memory_gb,
        "used_memory_gb": hw.used_memory_gb,
        "vram_gb": hw.vram_gb,
        "arch": hw.arch,
        "machine": hw.machine,
        "os_name": hw.os_name,
    }


def check_model_compatibility(model_key: str) -> tuple[bool, str]:
    """Delegates to `HardwareManager.check_model_compatibility`."""
    return HardwareManager.check_model_compatibility(model_key)


def check_speculative_compatibility(main_model_key: str, draft_model_key: str) -> tuple[bool, str]:
    """Delegates to `HardwareManager.check_speculative_compatibility`."""
    return HardwareManager.check_speculative_compatibility(main_model_key, draft_model_key)


__all__ = [
    "MODEL_MATRIX",
    "AdapterBaseDigest",
    "AdapterCatalog",
    "AdapterInstaller",
    "AdapterManifest",
    "AdapterSignature",
    "AdapterStore",
    "AdapterVerifier",
    "HardwareManager",
    "HardwareTelemetry",
    "MemoryMetrics",
    "ModelDownloader",
    "ModelMatrixRegistry",
    "ModelSpec",
    "ReleaseAssetDownloader",
    "check_model_compatibility",
    "check_speculative_compatibility",
    "detect_gb10_hardware",
    "download_all_models",
    "download_model",
    "download_model_with_drafter",
    "download_recipe_companions",
    "get_model_launch_overrides",
    "get_speculative_draft_repo",
    "get_system_memory",
    "get_tensorized_path",
    "is_model_downloaded",
    "is_model_tensorized",
    "model_declares_own_quantization",
    "model_is_diffusion",
    "model_key_for_served_id",
    "model_supports_vision",
    "resolve_model_hf_repo",
    "tensorize_model",
]
