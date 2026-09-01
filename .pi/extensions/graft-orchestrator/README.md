# Graft Pi orchestrator extension

Project-local Pi extension for durable ticket planning, isolated subagent runners,
automatic preflight, bounded review, and maintainer approval gates.

## Load check

From the repository root:

```console
pi --offline --no-session --no-extensions \
  -e ./.pi/extensions/graft-orchestrator/index.ts \
  -p --model openai-codex/gpt-5.4-mini 'reply ok'
```

The command must print `ok` without an extension error.

## Test suite

```console
node --test .pi/extensions/graft-orchestrator/test/*.test.mjs
```

## Safety

The daemon stores durable state outside the repository under
`~/.local/state/pi-orch/`. Do not restart or replace a running daemon merely to
validate source changes. Pushes, pull requests, merges, workflow dispatches,
tags, and releases remain separate maintainer-approved actions.
