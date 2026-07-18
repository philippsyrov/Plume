# Judge Testing

## Supported build

- macOS on Apple Silicon (arm64)
- Plume 0.1.0
- No source checkout or toolchain is required for the packaged golden path
- The current judge candidate is ad-hoc signed, not Developer ID signed or
  notarized

## Install

1. Download `Plume_0.1.0_aarch64.dmg` from the submission's release link.
2. Open the DMG and drag Plume into Applications.
3. Open Plume. If macOS blocks the unsigned build, open **System Settings →
   Privacy & Security** and choose **Open Anyway** for Plume.
4. A model is optional for the no-model evidence path below. To test chat,
   open **Model** before or after opening a disposable project.
5. Choose a disposable local project folder and approve trust when Plume asks.

The public release link is an owner action and must be added to the Devpost
submission after the final DMG is uploaded. The repository itself does not
pretend that a local build path is a judge download.

## Five-minute golden path

Use a small folder containing at least one readable text or code file.

1. Open the file in **Files** and choose **Use in chat**. Confirm that a File
   card appears above the composer.
2. Open **Browser**, visit `https://example.com`, then choose **Use page in
   chat**. Confirm that a Web card appears.
3. Open **Settings → Project memory**, save a short note such as "Keep this
   helper deterministic and easy to explain."
4. Open **Library**, find the project note, and choose **Use in chat**. Confirm
   that a Memory card appears.
5. Return to Chat. The File, Web, and Memory sources should remain individually
   visible. Open **Details** on each card to inspect its exact reference, or
   remove a source with its close button.
6. Quit and reopen Plume, reopen the same project, and confirm that the chat's
   selected context is restored.

This path works without downloading a model and demonstrates the qualifying
context, Browser evidence, Library, and persistence work.

## Optional model path

Open **Model** in the top bar. If **Apple On-Device** is available, select it
without a download; the status comes from the host Foundation Models framework
and unsupported or not-ready hosts stay disabled. Otherwise choose **Download**
for **Qwen Coder 1.5B**, optionally exercise Cancel/Resume, wait for verification,
then choose **Use Qwen**. Plume bundles the MLX-LM runtime, not the roughly
880 MB Qwen weights. The fixed weights download is explicit and stored in app
data; Ollama and user-managed Python are not required.

Ask a question using the three explicit sources. Normal chat is implicit.
Choose **Make changes** only when you want Plume to draft a file change; the
draft still requires an explicit **Apply**, and an applied patch can be reverted
through Plume's checkpointed path.

Model setup is not required for the no-download golden path above. The catalog
adds chat generation; it does not add broad tools or a multi-step coding agent.

## Honest boundaries

Plume does not ship broad shell/tool execution, agent-controlled Browser
actions, computer-use emission, semantic retrieval, or an autonomous
multi-iteration coding loop. The Browser is human-controlled, evidence is
attached explicitly, and file changes use the guarded patch flow.
