import unittest

from plume_mlx_vlm_policy import PolicyError, validate_chat_payload


PINNED_MODEL = "/private/app-data/models/catalog/qwen2-vl/revision"
PNG_DATA_URL = "data:image/png;base64,iVBORw0KGgo="


def payload(**overrides):
    value = {
        "model": PINNED_MODEL,
        "stream": True,
        "stream_options": {"include_usage": True},
        "max_tokens": 4096,
        "stop": ["<|im_end|>"],
        "messages": [
            {
                "role": "user",
                "content": [
                    {"type": "text", "text": "What is shown?"},
                    {"type": "image_url", "image_url": {"url": PNG_DATA_URL}},
                ],
            }
        ],
    }
    value.update(overrides)
    return value


class PlumeMlxVlmPolicyTests(unittest.TestCase):
    def test_accepts_the_exact_pinned_model_and_inline_png(self):
        validate_chat_payload(payload(), PINNED_MODEL)

    def test_rejects_model_switching(self):
        with self.assertRaisesRegex(PolicyError, "fixed preloaded model"):
            validate_chat_payload(payload(model="mlx-community/another-model"), PINNED_MODEL)

    def test_rejects_adapter_override_even_when_null(self):
        with self.assertRaisesRegex(PolicyError, "adapters"):
            validate_chat_payload(payload(adapter_path=None), PINNED_MODEL)

    def test_rejects_top_level_fields_that_rust_does_not_emit(self):
        for field, value in [
            ("temperature", 0.7),
            ("resize_shape", [2048, 2048]),
            ("logit_bias", {"1": 100}),
            ("response_format", {"type": "json_object"}),
            ("tools", []),
        ]:
            with self.subTest(field=field):
                with self.assertRaisesRegex(PolicyError, "unsupported request fields"):
                    validate_chat_payload(payload(**{field: value}), PINNED_MODEL)

    def test_bounds_the_supported_generation_controls(self):
        invalid_overrides = [
            {"max_tokens": 4097},
            {"max_tokens": 0},
            {"max_tokens": True},
            {"stream": False},
            {"stream_options": {"include_usage": False}},
            {"stop": ["different-stop"]},
        ]
        for overrides in invalid_overrides:
            with self.subTest(overrides=overrides):
                with self.assertRaisesRegex(PolicyError, "generation controls"):
                    validate_chat_payload(payload(**overrides), PINNED_MODEL)

    def test_rejects_empty_or_excessive_message_lists(self):
        with self.assertRaisesRegex(PolicyError, "between 1 and 256"):
            validate_chat_payload(payload(messages=[]), PINNED_MODEL)
        too_many = [{"role": "user", "content": "x"}] * 257
        with self.assertRaisesRegex(PolicyError, "between 1 and 256"):
            validate_chat_payload(payload(messages=too_many), PINNED_MODEL)

    def test_rejects_empty_messages_and_more_than_one_mibibyte_of_text(self):
        with self.assertRaisesRegex(PolicyError, "empty"):
            validate_chat_payload(
                payload(messages=[{"role": "user", "content": ""}]), PINNED_MODEL
            )
        oversized = "🦕" * ((1024 * 1024 // 4) + 1)
        with self.assertRaisesRegex(PolicyError, "1 MiB"):
            validate_chat_payload(
                payload(messages=[{"role": "user", "content": oversized}]),
                PINNED_MODEL,
            )

    def test_accepts_rust_tool_role_while_images_remain_user_only(self):
        request = payload(
            messages=[
                {"role": "tool", "content": "bounded tool result"},
                {"role": "user", "content": "summarize it"},
            ]
        )
        validate_chat_payload(request, PINNED_MODEL)

    def test_rejects_remote_images(self):
        request = payload()
        request["messages"][0]["content"][1]["image_url"]["url"] = (
            "https://example.com/image.png"
        )
        with self.assertRaisesRegex(PolicyError, "inline PNG"):
            validate_chat_payload(request, PINNED_MODEL)

    def test_rejects_non_png_data_images(self):
        request = payload()
        request["messages"][0]["content"][1]["image_url"]["url"] = (
            "data:image/jpeg;base64,/9j/4AAQ"
        )
        with self.assertRaisesRegex(PolicyError, "inline PNG"):
            validate_chat_payload(request, PINNED_MODEL)

    def test_rejects_malformed_png_base64(self):
        request = payload()
        request["messages"][0]["content"][1]["image_url"]["url"] = (
            "data:image/png;base64,not base64"
        )
        with self.assertRaisesRegex(PolicyError, "base64"):
            validate_chat_payload(request, PINNED_MODEL)


if __name__ == "__main__":
    unittest.main()
