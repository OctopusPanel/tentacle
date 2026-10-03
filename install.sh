#!/usr/bin/env bash
# ==============================================================================
#  🐙 Tentacle Installer - High-Performance Node Daemon for OctopusPanel
#  Specification: Concept/TENTACLE_INSTALLER.md
# ==============================================================================

set -eo pipefail
export LC_ALL=C.UTF-8

# ------------------------------------------------------------------------------
# Version & Defaults
# ------------------------------------------------------------------------------
INSTALLER_VERSION="v1.0.0-beta"
DEFAULT_TENTACLE_VERSION="v0.1.0"
DEFAULT_API_PORT=8080
DEFAULT_SFTP_PORT=2022
DEFAULT_STORAGE_PATH="/var/lib/octopus/volumes"
CONFIG_DIR="/etc/octopus"
CONFIG_FILE="${CONFIG_DIR}/tentacle.yaml"
LOG_DIR="/var/log/octopus"
LOG_FILE="${LOG_DIR}/tentacle-install.log"
BINARY_DIR="/usr/local/bin"
BINARY_PATH="${BINARY_DIR}/tentacle"
SYSTEMD_SERVICE_FILE="/etc/systemd/system/tentacle.service"
GITHUB_REPO="OctopusPanel/tentacle"

# ------------------------------------------------------------------------------
# Color Palette & Typography
# ------------------------------------------------------------------------------
if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
    CLR_RESET="\033[0m"
    CLR_BOLD="\033[1m"
    CLR_DIM="\033[2m"
    CLR_CYAN="\033[38;2;0;240;255m"      # #00F0FF / ANSI 14
    CLR_PURPLE="\033[38;2;189;147;249m"  # #BD93F9 / ANSI 13
    CLR_GREEN="\033[38;2;80;250;123m"    # #50FA7B / ANSI 10
    CLR_YELLOW="\033[38;2;241;250;140m"  # #F1FA8C / ANSI 11
    CLR_RED="\033[38;2;255;85;85m"       # #FF5555 / ANSI 9
    CLR_GRAY="\033[38;2;98;114;164m"     # #6272A4 / ANSI 8
else
    CLR_RESET=""
    CLR_BOLD=""
    CLR_DIM=""
    CLR_CYAN=""
    CLR_PURPLE=""
    CLR_GREEN=""
    CLR_YELLOW=""
    CLR_RED=""
    CLR_GRAY=""
fi

GLYPH_SUCCESS="${CLR_GREEN}✔${CLR_RESET}"
GLYPH_FAIL="${CLR_RED}✖${CLR_RESET}"
GLYPH_INFO="${CLR_CYAN}ℹ${CLR_RESET}"
GLYPH_WARN="${CLR_YELLOW}⚠${CLR_RESET}"
GLYPH_PROMPT="${CLR_PURPLE}➜${CLR_RESET}"

# ------------------------------------------------------------------------------
# Cleanup & Signal Handling
# ------------------------------------------------------------------------------
cleanup_terminal() {
    printf "\033[?25h" 2>/dev/null || true
}
trap cleanup_terminal EXIT INT TERM

# ------------------------------------------------------------------------------
# Visual Formatting Helpers
# ------------------------------------------------------------------------------
pad_box_line() {
    local text="$1"
    local target_len="${2:-60}"
    local char_len="${#text}"
    local pad=$(( target_len - char_len ))
    if [ "$pad" -gt 0 ]; then
        local spaces
        spaces=$(printf "%*s" "$pad" "")
        printf "%s%s" "${text}" "${spaces}"
    else
        printf "%s" "${text:0:$target_len}"
    fi
}

render_header() {
    local version="${1:-$INSTALLER_VERSION}"
    local arch="${2:-x86_64}"
    local v_line="      Version: ${version} | Architecture: ${arch}"
    local padded_v
    padded_v="$(pad_box_line "${v_line}" 60)"

    printf "\n"
    printf "    ${CLR_CYAN}╭────────────────────────────────────────────────────────────╮${CLR_RESET}\n"
    printf "    ${CLR_CYAN}│${CLR_PURPLE}  🐙  T E N T A C L E   I N S T A L L E R                   ${CLR_CYAN}│${CLR_RESET}\n"
    printf "    ${CLR_CYAN}│${CLR_RESET}      High-Performance Node Daemon for OctopusPanel         ${CLR_CYAN}│${CLR_RESET}\n"
    printf "    ${CLR_CYAN}│${CLR_GRAY}%s${CLR_CYAN}│${CLR_RESET}\n" "${padded_v}"
    printf "    ${CLR_CYAN}╰────────────────────────────────────────────────────────────╯${CLR_RESET}\n\n"
}

show_error_box() {
    local step="$1"
    local reason="$2"
    local details="${3:-}"

    printf "\n"
    local title_text="  ✖  INSTALLATION FAILED AT STEP ${step}"
    local padded_title
    padded_title="$(pad_box_line "${title_text}" 60)"

    printf "    ${CLR_RED}╭────────────────────────────────────────────────────────────╮${CLR_RESET}\n"
    printf "    ${CLR_RED}│${CLR_BOLD}%s${CLR_RESET}${CLR_RED}│${CLR_RESET}\n" "${padded_title}"
    printf "    ${CLR_RED}╰────────────────────────────────────────────────────────────╯${CLR_RESET}\n\n"

    printf "  ${CLR_BOLD}Reason:${CLR_RESET}  %s\n" "${reason}"
    if [ -n "${details}" ]; then
        printf "  ${CLR_BOLD}Details:${CLR_RESET} %s\n" "${details}"
    fi
    printf "\n"

    printf "  ${CLR_GRAY}┌── Diagnostic Log Snippet (Last 15 lines) ────────────────────┐${CLR_RESET}\n"
    if [ -f "${LOG_FILE}" ]; then
        local log_lines=()
        mapfile -t log_lines < <(tail -n 15 "${LOG_FILE}" 2>/dev/null || true)
        if [ ${#log_lines[@]} -eq 0 ]; then
            printf "  ${CLR_GRAY}│${CLR_RESET} %-60s ${CLR_GRAY}│${CLR_RESET}\n" "(No logs recorded yet)"
        else
            for line in "${log_lines[@]}"; do
                local clean_line
                clean_line=$(echo "${line}" | tr -d '\r' | cut -c 1-60)
                printf "  ${CLR_GRAY}│${CLR_RESET} %-60s ${CLR_GRAY}│${CLR_RESET}\n" "${clean_line}"
            done
        fi
    else
        printf "  ${CLR_GRAY}│${CLR_RESET} %-60s ${CLR_GRAY}│${CLR_RESET}\n" "(Log file not found: ${LOG_FILE})"
    fi
    printf "  ${CLR_GRAY}└──────────────────────────────────────────────────────────────┘${CLR_RESET}\n\n"

    printf "  ${CLR_BOLD}Full installation log available at:${CLR_RESET}\n"
    printf "  %s\n\n" "${LOG_FILE}"
    printf "  Need help? Open an issue at ${CLR_CYAN}https://github.com/${GITHUB_REPO}${CLR_RESET}\n\n"
}

show_success_box() {
    local api_port="$1"
    local sftp_port="$2"
    local config_file="$3"

    printf "\n"
    local title_text="  ✔  TENTACLE INSTALLED & RUNNING SUCCESSFULLY!"
    local padded_title
    padded_title="$(pad_box_line "${title_text}" 60)"

    printf "    ${CLR_GREEN}╭────────────────────────────────────────────────────────────╮${CLR_RESET}\n"
    printf "    ${CLR_GREEN}│${CLR_BOLD}%s${CLR_RESET}${CLR_GREEN}│${CLR_RESET}\n" "${padded_title}"
    printf "    ${CLR_GREEN}╰────────────────────────────────────────────────────────────╯${CLR_RESET}\n\n"

    printf "  • ${CLR_BOLD}Daemon Status:${CLR_RESET}     ${CLR_GREEN}Active (running)${CLR_RESET}\n"
    printf "  • ${CLR_BOLD}API Endpoint:${CLR_RESET}      ${CLR_CYAN}http://0.0.0.0:%s${CLR_RESET}\n" "${api_port}"
    printf "  • ${CLR_BOLD}SFTP Port:${CLR_RESET}         ${CLR_CYAN}%s${CLR_RESET}\n" "${sftp_port}"
    printf "  • ${CLR_BOLD}Configuration:${CLR_RESET}     %s\n" "${config_file}"
    printf "  • ${CLR_BOLD}Service Logs:${CLR_RESET}      ${CLR_PURPLE}journalctl -u tentacle -f${CLR_RESET}\n\n"
    printf "  Your node is now ready to receive server containers from OctopusPanel.\n\n"
}

# ------------------------------------------------------------------------------
# Progress Spinner & Step Execution
# ------------------------------------------------------------------------------
run_step() {
    local step_label="$1"
    shift
    local cmd=("$@")

    mkdir -p "${LOG_DIR}" 2>/dev/null || true
    touch "${LOG_FILE}" 2>/dev/null || true
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] Starting: ${step_label}" >> "${LOG_FILE}"

    if [ ! -t 1 ]; then
        printf "  %s  %s ...\n" "${GLYPH_INFO}" "${step_label}"
        if "${cmd[@]}" >> "${LOG_FILE}" 2>&1; then
            printf "  %s  %s\n" "${GLYPH_SUCCESS}" "${step_label}"
            echo "[$(date '+%Y-%m-%d %H:%M:%S')] Completed: ${step_label}" >> "${LOG_FILE}"
            return 0
        else
            local exit_code=$?
            printf "  %s  %s (failed)\n" "${GLYPH_FAIL}" "${step_label}"
            echo "[$(date '+%Y-%m-%d %H:%M:%S')] Failed (exit code ${exit_code}): ${step_label}" >> "${LOG_FILE}"
            return "${exit_code}"
        fi
    fi

    local spinstr=('⠋' '⠙' '⠹' '⠸' '⠼' '⠴' '⠦' '⠧' '⠇' '⠏')
    local delay=0.08
    local pid

    "${cmd[@]}" >> "${LOG_FILE}" 2>&1 &
    pid=$!

    printf "\033[?25l"

    local i=0
    while kill -0 "${pid}" 2>/dev/null; do
        printf "\r  ${CLR_CYAN}%s${CLR_RESET}  %s" "${spinstr[i]}" "${step_label}"
        i=$(( (i + 1) % ${#spinstr[@]} ))
        sleep "${delay}"
    done

    wait "${pid}"
    local exit_code=$?

    printf "\033[?25h"

    if [ "${exit_code}" -eq 0 ]; then
        printf "\r  %s  %s\033[K\n" "${GLYPH_SUCCESS}" "${step_label}"
        echo "[$(date '+%Y-%m-%d %H:%M:%S')] Completed: ${step_label}" >> "${LOG_FILE}"
        return 0
    else
        printf "\r  %s  %s\033[K\n" "${GLYPH_FAIL}" "${step_label}"
        echo "[$(date '+%Y-%m-%d %H:%M:%S')] Failed (exit code ${exit_code}): ${step_label}" >> "${LOG_FILE}"
        return "${exit_code}"
    fi
}
