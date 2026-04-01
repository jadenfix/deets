#!/usr/bin/env bash
# ============================================================================
# PreToolUse safety hook for Aether blockchain Claude Code harness
# ============================================================================
# Defense-in-depth layer. Reads JSON from stdin (Claude Code hook protocol).
# Returns JSON with permissionDecision:"deny" on stdout to block.
# No output + exit 0 = allow.
# ============================================================================

set -euo pipefail

INPUT="$(cat)"
TOOL_NAME="$(echo "$INPUT" | jq -r '.tool_name // empty')"
COMMAND="$(echo "$INPUT" | jq -r '.tool_input.command // empty')"

if [ "$TOOL_NAME" != "Bash" ] || [ -z "$COMMAND" ]; then
    exit 0
fi

deny() {
    cat <<DENYEOF
{
  "hookSpecificOutput": {
    "hookEventName": "PreToolUse",
    "permissionDecision": "deny",
    "permissionDecisionReason": "$1"
  }
}
DENYEOF
    exit 0
}

# ── Rule 1: Block git push --force / -f ──
if echo "$COMMAND" | grep -qE 'git\s+push\s+.*(-f\b|--force)'; then
    deny "Force-push is prohibited in unattended mode"
fi

# ── Rule 2: Block rm -rf outside repo and /tmp/claude-build ──
if echo "$COMMAND" | grep -qE 'rm\s+-r[f ]*\s' ; then
    if ! echo "$COMMAND" | grep -qE 'rm\s+-r[f ]+\s*(\./|node_modules|target|data|dist|build|/tmp/claude-build)'; then
        deny "rm -rf is only allowed within the repo directory or /tmp/claude-build"
    fi
fi

# ── Rule 3: Block reading sensitive credential directories ──
# Match both literal ~ and expanded $HOME paths
SENSITIVE_PATTERNS=(
    '~/.ssh'
    '~/.aws'
    '~/.gnupg'
    '~/.config/gcloud'
    '~/.kube/config'
    "$HOME/.ssh"
    "$HOME/.aws"
    "$HOME/.gnupg"
    "$HOME/.config/gcloud"
    "$HOME/.kube/config"
    '/Users/jadenfix/.ssh'
    '/Users/jadenfix/.aws'
    '/Users/jadenfix/.gnupg'
)
for SPATH in "${SENSITIVE_PATTERNS[@]}"; do
    if echo "$COMMAND" | grep -qF "$SPATH"; then
        deny "Access to sensitive directory ${SPATH} is prohibited"
    fi
done

# ── Rule 4: Block reading repo secrets ──
if echo "$COMMAND" | grep -qE '(cat|head|tail|less|more|bat|xxd|hexdump)\s+.*keys/'; then
    deny "Reading key material from keys/ is prohibited"
fi
if echo "$COMMAND" | grep -qE '(cat|head|tail|less|more|bat)\s+.*\.env'; then
    deny "Reading .env files is prohibited"
fi
if echo "$COMMAND" | grep -qE '(cat|head|tail|less|more|bat)\s+.*\.(pem|key)\b'; then
    deny "Reading PEM/key files is prohibited"
fi
if echo "$COMMAND" | grep -qE '(cat|head|tail|less|more|bat)\s+.*\.tfstate'; then
    deny "Reading Terraform state files is prohibited"
fi

# ── Rule 5: Block curl/wget to non-whitelisted domains ──
ALLOWED_DOMAINS="127.0.0.1 localhost github.com api.github.com crates.io static.crates.io index.crates.io registry.npmjs.org pypi.org files.pythonhosted.org docs.rs doc.rust-lang.org docs.anthropic.com"

if echo "$COMMAND" | grep -qE '(curl|wget)\s'; then
    URLS="$(echo "$COMMAND" | grep -oE 'https?://[^ "'"'"']+' || true)"
    if [ -n "$URLS" ]; then
        while IFS= read -r url; do
            DOMAIN="$(echo "$url" | sed -E 's|^https?://([^/:]+).*|\1|')"
            ALLOWED=false
            for AD in $ALLOWED_DOMAINS; do
                if [ "$DOMAIN" = "$AD" ]; then
                    ALLOWED=true
                    break
                fi
            done
            if [ "$ALLOWED" = false ]; then
                deny "curl/wget to non-whitelisted domain: ${DOMAIN}"
            fi
        done <<< "$URLS"
    fi
fi

# ── Rule 6: Block publishing ──
if echo "$COMMAND" | grep -qE '(cargo\s+publish|npm\s+publish|twine\s+upload)'; then
    deny "Publishing packages is prohibited in unattended mode"
fi

# ── Rule 7: Block deployment commands ──
if echo "$COMMAND" | grep -qE '(kubectl\s+(apply|delete)|helm\s+(install|upgrade)|terraform\s+(apply|destroy))'; then
    deny "Deployment commands are prohibited in unattended mode"
fi

# ── Rule 8: Block downloading arbitrary binaries ──
if echo "$COMMAND" | grep -qE 'curl\s+.*-[a-zA-Z]*o'; then
    deny "Downloading files with curl -o is prohibited"
fi
if echo "$COMMAND" | grep -qE '\bwget\b'; then
    deny "wget is prohibited in unattended mode"
fi

exit 0
