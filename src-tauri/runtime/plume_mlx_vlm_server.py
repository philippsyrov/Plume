"""Plume launcher for the pinned MLX-VLM server.

MLX-VLM 0.5.0's continuous-batching generator does not complete Qwen2-VL
image requests on Plume's supported path. Keep its request type and completion
implementation, but expose only Plume's fixed-model health and chat routes and
use the existing ``stream_generate`` fallback.
"""

import os
from contextlib import asynccontextmanager

from fastapi import FastAPI, HTTPException, Request
from mlx_vlm import server as upstream

from plume_mlx_vlm_policy import PolicyError, validate_chat_payload


_pinned_model_path = None


def get_cached_model_without_batching(
    model_path, adapter_path=upstream._INHERIT_ADAPTER
):
    """Return only Plume's preloaded model, without a ResponseGenerator."""
    if not _pinned_model_path or model_path != _pinned_model_path:
        raise ValueError("only Plume's fixed preloaded model is available")
    if adapter_path is upstream._INHERIT_ADAPTER:
        adapter_path = None
    if adapter_path is not None:
        raise ValueError("model adapters are not available")

    cache_key = (model_path, adapter_path)
    if upstream.model_cache.get("cache_key") == cache_key:
        return (
            upstream.model_cache["model"],
            upstream.model_cache["processor"],
            upstream.model_cache["config"],
        )

    if upstream.model_cache:
        upstream.unload_model_sync()

    vision_cache_size = int(os.environ.get("MLX_VLM_VISION_CACHE_SIZE", "20"))
    vision_cache = upstream.VisionFeatureCache(max_size=vision_cache_size)
    upstream.apc_manager = upstream._apc.from_env(model_namespace=model_path)
    model, processor, config = upstream.load_model_resources(model_path, adapter_path)
    upstream.response_generator = None
    upstream.model_cache = {
        "cache_key": cache_key,
        "model_path": model_path,
        "adapter_path": adapter_path,
        "model": model,
        "processor": processor,
        "config": config,
        "vision_cache": vision_cache,
    }
    return model, processor, config


@asynccontextmanager
async def lifespan_without_batching(_app):
    """Preload the requested model while keeping direct generation selected."""
    global _pinned_model_path
    model_path = os.environ.pop("MLX_VLM_PRELOAD_MODEL", None)
    if not model_path:
        raise RuntimeError("Plume's MLX-VLM server requires a fixed --model path")
    if os.environ.pop("MLX_VLM_PRELOAD_ADAPTER", None) is not None:
        raise RuntimeError("Plume's MLX-VLM server does not accept adapters")
    _pinned_model_path = model_path
    upstream.logger.info("Pre-loading fixed model without continuous batching: %s", model_path)
    get_cached_model_without_batching(model_path)
    upstream.logger.info("Model ready, direct streaming enabled.")
    yield


upstream.get_cached_model = get_cached_model_without_batching
upstream.response_generator = None

secured_app = FastAPI(
    title="Plume MLX-VLM Runtime",
    docs_url=None,
    redoc_url=None,
    openapi_url=None,
    lifespan=lifespan_without_batching,
)


@secured_app.get("/health")
async def health_check():
    """Expose the supervisor's only readiness probe."""
    return await upstream.health_check()


@secured_app.post("/chat/completions", response_model=None)
@secured_app.post("/v1/chat/completions", response_model=None)
async def secured_chat_completions(
    request: upstream.ChatRequest, http_request: Request
):
    """Validate Plume's narrow request boundary before upstream generation."""
    try:
        validate_chat_payload(request.model_dump(exclude_unset=True), _pinned_model_path)
    except PolicyError as error:
        raise HTTPException(status_code=400, detail=str(error)) from error
    return await upstream.chat_completions_endpoint(request, http_request)


# Upstream's launcher imports this module-level name by string. Replace it only
# after all reused endpoint functions have been captured above.
upstream.app = secured_app


if __name__ == "__main__":
    upstream.main()
