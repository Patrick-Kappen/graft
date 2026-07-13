# Security policy

## Project status

Graft is early-alpha software. Its current releases prove the rootfs-store
materialisation path and document its current threat model; they do not yet
provide secure defaults or a production support policy.

Security fixes are handled on a best-effort basis against `main` and the latest
pre-release. Older pre-releases are not guaranteed to receive backports. No
response or remediation service-level agreement is offered.

## Report a vulnerability privately

Do not open a public issue for a suspected vulnerability.

Open the repository's [Security page](https://github.com/Patrick-Kappen/graft/security)
and choose **Report a vulnerability** to send the report only to the repository
maintainers. If GitHub does not offer the private form, do not substitute a
public issue; wait until the private route is available again.

Examples of security-sensitive findings include:

- command, systemd, Quadlet, or generated-unit injection;
- privilege escalation or an unexpected rootful execution path;
- bypass of typed capability, host-policy, mount, path, or target boundaries;
- cross-workload access caused by identity, ownership, cleanup, or concurrency
  errors;
- secret or credential disclosure caused by Graft output or lifecycle behavior;
- unsafe path traversal, symlink handling, or promotion behavior;
- a dependency or workflow vulnerability with a demonstrated impact on Graft's
  build, release, or runtime trust boundary.

Ordinary validation errors, documentation mistakes, feature requests, and
unsupported configurations can use the public issue forms unless they expose a
security boundary.

## What to include

Provide only the minimum sanitized information needed to reproduce and assess
the issue:

- affected Graft release or commit;
- NixOS system or Home Manager user scope;
- system/rootful or user/rootless target;
- architecture and relevant Nixpkgs, Podman, Quadlet, and systemd versions;
- a minimal sanitized TOML or generated-unit fragment;
- reproduction steps, expected behavior, and observed security impact;
- whether the issue is already being exploited or publicly known.

Never include live credentials, API keys, tokens, private repository contents,
private hostnames, internal endpoints, personal data, unrestricted environment
dumps, or complete logs. Replace sensitive values with clear placeholders.

## Isolation and trust boundaries

Rootless containers are the preferred direction for unattended server
workloads, but containers share the host kernel and are not a VM-equivalent
security boundary. System-target workloads are rootful. Repository intent must
remain constrained by trusted host and security policy.

The current assumptions, invariants, and accepted residual risks are defined in
[Threat model and trust boundaries](docs/threat-model.md). Secure defaults and
capability policy remain active work in the
[security roadmap](docs/roadmap.md#security-hardening). Do not infer
unimplemented isolation guarantees from future roadmap or vision text.

## Disclosure and response expectations

Maintainers may ask for clarification through the private advisory. Please keep
the report and follow-up discussion private until a coordinated disclosure or
fix is agreed. The project may decline reports that lack a Graft-specific impact
or concern behavior explicitly outside the supported current scope.
