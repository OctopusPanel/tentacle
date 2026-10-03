#!/usr/bin/env bash
# ==============================================================================
#  Tests for Tentacle Installation Script (install.sh)
# ==============================================================================

set -eo pipefail
export LC_ALL=C.UTF-8

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
INSTALLER="${SCRIPT_DIR}/../install.sh"
PASS=0
FAIL=0

assert_eq() {
    local expected="$1"
    local actual="$2"
    local test_name="$3"
    if [ "$expected" = "$actual" ]; then
        printf "  \033[32m✔\033[0m %s\n" "$test_name"
        PASS=$((PASS + 1))
    else
        printf "  \033[31m✖\033[0m %s (Expected: '%s', Got: '%s')\n" "$test_name" "$expected" "$actual"
        FAIL=$((FAIL + 1))
    fi
}

assert_contains() {
    local needle="$1"
    local haystack="$2"
    local test_name="$3"
    if [[ "$haystack" == *"$needle"* ]]; then
        printf "  \033[32m✔\033[0m %s\n" "$test_name"
        PASS=$((PASS + 1))
    else
        printf "  \033[31m✖\033[0m %s (Did not contain '%s')\n" "$test_name" "$needle"
        FAIL=$((FAIL + 1))
    fi
}

echo "Running Tentacle Installer Test Suite..."

# ------------------------------------------------------------------------------
# Test 1: Bash syntax check
# ------------------------------------------------------------------------------
if bash -n "$INSTALLER"; then
    printf "  \033[32m✔\033[0m Syntax validation (bash -n)\n"
    PASS=$((PASS + 1))
else
    printf "  \033[31m✖\033[0m Syntax validation failed\n"
    FAIL=$((FAIL + 1))
fi

# ------------------------------------------------------------------------------
# Test 2: Help flag output
# ------------------------------------------------------------------------------
HELP_OUTPUT="$("$INSTALLER" --help 2>&1 || true)"
assert_contains "T E N T A C L E   I N S T A L L E R" "$HELP_OUTPUT" "Help renders ASCII header"
assert_contains "--panel-url" "$HELP_OUTPUT" "Help documents --panel-url"
assert_contains "--token" "$HELP_OUTPUT" "Help documents --token"
assert_contains "--install-docker" "$HELP_OUTPUT" "Help documents --install-docker"
assert_contains "--unattended" "$HELP_OUTPUT" "Help documents --unattended"

# ------------------------------------------------------------------------------
# Test 3: Root privilege rejection and error box
# ------------------------------------------------------------------------------
# In unprivileged environment, running without root should fail with code 1
RUN_OUTPUT="$("$INSTALLER" 2>&1 || true)"
assert_contains "INSTALLATION FAILED AT STEP [1/6]" "$RUN_OUTPUT" "Error box displayed on unprivileged run"
assert_contains "Root privileges required" "$RUN_OUTPUT" "Error reason mentions root privileges"

# ------------------------------------------------------------------------------
# Test 4: CLI argument parsing & variable setting
# ------------------------------------------------------------------------------
PARSE_TEST_OUTPUT="$(bash -c "
    source <(sed -e '/main \"\$@\"/d' -e '/trap cleanup_terminal/d' '$INSTALLER')
    parse_args --panel-url 'https://test.octopuspanel.com' \
               --token 'sec_tok_12345' \
               --api-port 9090 \
               --sftp-port 2023 \
               --storage-path '/custom/vols' \
               --node-id 'node-test-99' \
               --node-name 'Custom Test Node' \
               --install-docker \
               --configure-firewall \
               --unattended
    echo \"URL:\$PANEL_URL|TOK:\$NODE_TOKEN|API:\$API_PORT|SFTP:\$SFTP_PORT|STOR:\$STORAGE_PATH|ID:\$NODE_ID|NAME:\$NODE_NAME|DOCK:\$OPT_INSTALL_DOCKER|FW:\$OPT_CONFIGURE_FIREWALL|UNAT:\$UNATTENDED\"
")"
EXPECTED_PARSE="URL:https://test.octopuspanel.com|TOK:sec_tok_12345|API:9090|SFTP:2023|STOR:/custom/vols|ID:node-test-99|NAME:Custom Test Node|DOCK:true|FW:true|UNAT:true"
assert_eq "$EXPECTED_PARSE" "$PARSE_TEST_OUTPUT" "CLI argument parsing sets all options correctly"

# ------------------------------------------------------------------------------
# Test 5: Quick input / 1-click token string parser
# ------------------------------------------------------------------------------
QUICK_TEST_OUTPUT="$(bash -c "
    source <(sed -e '/main \"\$@\"/d' -e '/trap cleanup_terminal/d' '$INSTALLER')
    parse_quick_input 'curl -sSL https://get.octopuspanel.com/tentacle/install.sh | sudo bash -s -- --panel-url \"https://panel.quick.com\" --token \"oct_node_sec_999\" --api-port 8888'
    echo \"URL:\$PANEL_URL|TOK:\$NODE_TOKEN|PORT:\$API_PORT\"
")"
assert_eq "URL:https://panel.quick.com|TOK:oct_node_sec_999|PORT:8888" "$QUICK_TEST_OUTPUT" "Quick input parses curl pipeline string"

QUICK_TEST_2="$(bash -c "
    source <(sed -e '/main \"\$@\"/d' -e '/trap cleanup_terminal/d' '$INSTALLER')
    parse_quick_input 'https://direct.panel.io oct_token_direct_secret'
    echo \"URL:\$PANEL_URL|TOK:\$NODE_TOKEN\"
")"
assert_eq "URL:https://direct.panel.io|TOK:oct_token_direct_secret" "$QUICK_TEST_2" "Quick input parses space-separated URL and token"

# ------------------------------------------------------------------------------
# Test 6: Configuration YAML generation
# ------------------------------------------------------------------------------
TMP_CONF_DIR="$(mktemp -d /tmp/tentacle-test-conf.XXXXXX)"
TMP_CONF_FILE="${TMP_CONF_DIR}/tentacle.yaml"

bash -c "
    source <(sed -e '/main \"\$@\"/d' -e '/trap cleanup_terminal/d' '$INSTALLER')
    LOG_DIR='${TMP_CONF_DIR}'
    LOG_FILE='${TMP_CONF_DIR}/test.log'
    CONFIG_FILE='${TMP_CONF_FILE}'
    CONFIG_DIR='${TMP_CONF_DIR}'
    DEFAULT_BACKUPS_PATH='/var/lib/octopus/backups'
    DEFAULT_TMP_PATH='/var/lib/octopus/tmp'
    PANEL_URL='https://panel.myhost.com'
    NODE_TOKEN='secret_jwt_key_98765'
    API_PORT=8080
    SFTP_PORT=2022
    STORAGE_PATH='/var/lib/octopus/volumes'
    NODE_ID='node-myhost-01'
    NODE_NAME='MyHost Production Node'
    UNATTENDED=true
    stage_config_wizard >/dev/null 2>&1
"

if [ -f "$TMP_CONF_FILE" ]; then
    assert_contains "panel_secret: \"secret_jwt_key_98765\"" "$(cat "$TMP_CONF_FILE")" "Config YAML contains auth secret"
    assert_contains "base_url: \"https://panel.myhost.com\"" "$(cat "$TMP_CONF_FILE")" "Config YAML contains panel base URL"
    assert_contains "listen_port: 8080" "$(cat "$TMP_CONF_FILE")" "Config YAML contains API port"
    assert_contains "listen_port: 2022" "$(cat "$TMP_CONF_FILE")" "Config YAML contains SFTP port"
    rm -rf "$TMP_CONF_DIR"
else
    printf "  \033[31m✖\033[0m Config file was not generated\n"
    FAIL=$((FAIL + 1))
fi

# ------------------------------------------------------------------------------
# Test 7: Systemd unit file template
# ------------------------------------------------------------------------------
TMP_SYS_DIR="$(mktemp -d /tmp/tentacle-test-sys.XXXXXX)"
TMP_SERVICE_FILE="${TMP_SYS_DIR}/tentacle.service"

bash -c "
    source <(sed -e '/main \"\$@\"/d' -e '/trap cleanup_terminal/d' '$INSTALLER')
    LOG_DIR='${TMP_SYS_DIR}'
    LOG_FILE='${TMP_SYS_DIR}/test.log'
    SYSTEMD_SERVICE_FILE='${TMP_SERVICE_FILE}'
    CONFIG_DIR='/etc/octopus'
    CONFIG_FILE='/etc/octopus/tentacle.yaml'
    BINARY_PATH='/usr/local/bin/tentacle'
    GITHUB_REPO='OctopusPanel/tentacle'
    # Override systemctl to avoid failing outside root
    systemctl() { return 0; }
    setup_systemd_service >/dev/null 2>&1
"

if [ -f "$TMP_SERVICE_FILE" ]; then
    assert_contains "ExecStart=/usr/local/bin/tentacle --config /etc/octopus/tentacle.yaml" "$(cat "$TMP_SERVICE_FILE")" "Systemd unit ExecStart configured"
    assert_contains "Restart=always" "$(cat "$TMP_SERVICE_FILE")" "Systemd unit Restart=always set"
    assert_contains "LimitNOFILE=65536" "$(cat "$TMP_SERVICE_FILE")" "Systemd unit LimitNOFILE set"
    rm -rf "$TMP_SYS_DIR"
else
    printf "  \033[31m✖\033[0m Systemd service file was not generated\n"
    FAIL=$((FAIL + 1))
fi

# ------------------------------------------------------------------------------
# Test 8: Wrapper script delegation
# ------------------------------------------------------------------------------
WRAPPER_HELP="$("${SCRIPT_DIR}/../scripts/install.sh" --help 2>&1 || true)"
assert_contains "T E N T A C L E   I N S T A L L E R" "$WRAPPER_HELP" "Wrapper script scripts/install.sh delegates properly"

# ------------------------------------------------------------------------------
# Summary
# ------------------------------------------------------------------------------
echo ""
echo "Test Results: ${PASS} Passed, ${FAIL} Failed."
if [ "$FAIL" -gt 0 ]; then
    exit 1
fi
exit 0
