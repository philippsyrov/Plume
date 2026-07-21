"""Fail-closed request policy for Plume's fixed MLX-VLM runtime."""

import base64
import binascii


PNG_DATA_URL_PREFIX = "data:image/png;base64,"
PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"
MAX_IMAGE_BYTES = 4 * 1024 * 1024
MAX_IMAGES = 10
MAX_MESSAGES = 256
MAX_TEXT_BYTES = 1024 * 1024
MAX_OUTPUT_TOKENS = 4096
QWEN_STOP_SEQUENCE = "<|im_end|>"
ALLOWED_REQUEST_FIELDS = {
    "model",
    "messages",
    "stream",
    "stream_options",
    "max_tokens",
    "stop",
}


class PolicyError(ValueError):
    """The localhost request exceeded Plume's fixed runtime authority."""


def validate_chat_payload(payload, pinned_model_path):
    """Accept only Plume's preloaded model and bounded inline PNG inputs."""
    if not isinstance(payload, dict):
        raise PolicyError("request body must be an object")
    if not pinned_model_path or payload.get("model") != pinned_model_path:
        raise PolicyError("only the fixed preloaded model is available")
    if "adapter_path" in payload:
        raise PolicyError("model adapters are not available")
    unsupported = set(payload) - ALLOWED_REQUEST_FIELDS
    missing = ALLOWED_REQUEST_FIELDS - set(payload)
    if unsupported or missing:
        raise PolicyError("unsupported request fields or missing required fields")
    if (
        payload.get("stream") is not True
        or payload.get("stream_options") != {"include_usage": True}
        or type(payload.get("max_tokens")) is not int
        or not 1 <= payload["max_tokens"] <= MAX_OUTPUT_TOKENS
        or payload.get("stop") != [QWEN_STOP_SEQUENCE]
    ):
        raise PolicyError("generation controls do not match Plume's bounded request")

    messages = payload.get("messages")
    if not isinstance(messages, list):
        raise PolicyError("messages must be a list")
    if not 1 <= len(messages) <= MAX_MESSAGES:
        raise PolicyError(f"messages must contain between 1 and {MAX_MESSAGES} items")

    image_count = 0
    total_text_bytes = 0
    for message in messages:
        if not isinstance(message, dict):
            raise PolicyError("each message must be an object")
        if set(message) != {"role", "content"}:
            raise PolicyError("message fields do not match Plume's request")
        role = message.get("role")
        if role not in ("system", "user", "assistant", "tool"):
            raise PolicyError("message role is not available")
        content = message.get("content")
        if isinstance(content, str):
            message_text_bytes = len(content.encode("utf-8"))
            if message_text_bytes == 0:
                raise PolicyError("messages cannot contain empty text")
            total_text_bytes += message_text_bytes
            if total_text_bytes > MAX_TEXT_BYTES:
                raise PolicyError("message text exceeds the 1 MiB total cap")
            continue
        if not isinstance(content, list):
            raise PolicyError("message content must be text or typed parts")
        message_text_bytes = 0
        for item in content:
            if not isinstance(item, dict):
                raise PolicyError("message content parts must be objects")
            item_type = item.get("type")
            if item_type in ("text", "input_text"):
                if set(item) != {"type", "text"} or not isinstance(item.get("text"), str):
                    raise PolicyError("text content fields are invalid")
                message_text_bytes += len(item["text"].encode("utf-8"))
                continue
            if item_type == "image_url":
                image = item.get("image_url")
                if set(item) != {"type", "image_url"} or not isinstance(image, dict):
                    raise PolicyError("image content fields are invalid")
                if set(image) != {"url"}:
                    raise PolicyError("image URL fields are invalid")
                image_url = image.get("url") if isinstance(image, dict) else None
            elif item_type == "input_image":
                if set(item) != {"type", "image_url"}:
                    raise PolicyError("image content fields are invalid")
                image_url = item.get("image_url")
            else:
                raise PolicyError("only text and inline PNG image parts are available")
            if role != "user":
                raise PolicyError("inline PNG images are accepted only in user messages")
            _validate_png_data_url(image_url)
            image_count += 1
            if image_count > MAX_IMAGES:
                raise PolicyError(f"at most {MAX_IMAGES} inline PNG images are available")
        if message_text_bytes == 0:
            raise PolicyError("messages cannot contain empty text")
        total_text_bytes += message_text_bytes
        if total_text_bytes > MAX_TEXT_BYTES:
            raise PolicyError("message text exceeds the 1 MiB total cap")


def _validate_png_data_url(image_url):
    if not isinstance(image_url, str) or not image_url.startswith(PNG_DATA_URL_PREFIX):
        raise PolicyError("images must be inline PNG data URLs")
    encoded = image_url[len(PNG_DATA_URL_PREFIX) :]
    try:
        decoded = base64.b64decode(encoded, validate=True)
    except (binascii.Error, ValueError) as error:
        raise PolicyError("inline PNG data must be valid base64") from error
    if not decoded.startswith(PNG_SIGNATURE):
        raise PolicyError("inline image data is not a PNG")
    if len(decoded) > MAX_IMAGE_BYTES:
        raise PolicyError("inline PNG exceeds the 4 MiB cap")
