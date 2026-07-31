# Project Rules

## Shared Assistant And Deployment Rules

- Project operating rules and assistant-facing configuration must support both Codex and Claude Code. When changing rules, hooks, skills, commands, or conventions for one assistant, update the matching configuration for the other assistant in the same change; do not land assistant-specific behavior unless the user explicitly asks for it.
- Deployment scripts must use explicit `ssh user@hostname` and `scp user@hostname:path` forms with hostnames or host aliases. Do not hard-code raw IP addresses in deployment commands; put host aliases in SSH config or project configuration instead.
- Deployment scripts must not generate long-lived systemd units or OpenWrt procd init scripts from heredocs, checked-in templates, or checked-in init files. If a service file must be created or migrated once, generate it with a temporary command and install it directly on the target host, then remove the generator/template from the repository.
