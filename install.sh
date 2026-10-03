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
DEFAULT_BACKUPS_PATH="/var/lib/octopus/backups"
DEFAULT_TMP_PATH="/var/lib/octopus/tmp"
CONFIG_DIR="/etc/octopus"
CONFIG_FILE="${CONFIG_DIR}/tentacle.yaml"
LOG_DIR="/var/log/octopus"
LOG_FILE="${LOG_DIR}/tentacle-install.log"
BINARY_DIR="/usr/local/bin"
BINARY_PATH="${BINARY_DIR}/tentacle"
SYSTEMD_SERVICE_FILE="/etc/systemd/system/tentacle.service"
GITHUB_REPO="OctopusPanel/tentacle"

# ------------------------------------------------------------------------------
# CLI State & Options
# ------------------------------------------------------------------------------
PANEL_URL=""
NODE_TOKEN=""
API_PORT=""
SFTP_PORT=""
STORAGE_PATH=""
NODE_ID=""
NODE_NAME=""
OPT_INSTALL_DOCKER=false
OPT_CONFIGURE_FIREWALL=false
UNATTENDED=false
LOCAL_BINARY=""
TARGET_VERSION="${DEFAULT_TENTACLE_VERSION}"

# Detection State
OS_FAMILY=""
DISTRO_ID=""
DISTRO_NAME=""
PKG_MANAGER=""
ARCH=""
TARGET_ARCH=""

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

    # Ensure log directory and file exist
    mkdir -p "${LOG_DIR}" 2>/dev/null || true
    touch "${LOG_FILE}" 2>/dev/null || true
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] Starting: ${step_label}" >> "${LOG_FILE}"

    if [ ! -t 1 ]; then
        # Headless / Non-interactive CI output
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

    # Animated interactive spinner
    local spinstr=('⠋' '⠙' '⠹' '⠸' '⠼' '⠴' '⠦' '⠧' '⠇' '⠏')
    local delay=0.08
    local pid

    # Run command in background redirected to log
    "${cmd[@]}" >> "${LOG_FILE}" 2>&1 &
    pid=$!

    # Hide cursor
    printf "\033[?25l"

    local i=0
    while kill -0 "${pid}" 2>/dev/null; do
        printf "\r  ${CLR_CYAN}%s${CLR_RESET}  %s" "${spinstr[i]}" "${step_label}"
        i=$(( (i + 1) % ${#spinstr[@]} ))
        sleep "${delay}"
    done

    wait "${pid}"
    local exit_code=$?

    # Restore cursor
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

# ------------------------------------------------------------------------------
# CLI Flag Parsing & Help
# ------------------------------------------------------------------------------
show_help() {
    render_header "${INSTALLER_VERSION}" "any"
    printf "Usage: %s [OPTIONS]\n\n" "$0"
    printf "Options:\n"
    printf "  --panel-url <url>        Full URL of the OctopusPanel master instance\n"
    printf "  --token <token>          Node registration or HMAC secret token\n"
    printf "  --api-port <port>        REST and WebSocket port (Default: %s)\n" "${DEFAULT_API_PORT}"
    printf "  --sftp-port <port>       Built-in SFTP server port (Default: %s)\n" "${DEFAULT_SFTP_PORT}"
    printf "  --storage-path <path>    Data directory for server containers (Default: %s)\n" "${DEFAULT_STORAGE_PATH}"
    printf "  --node-id <id>           Unique Node ID identifier\n"
    printf "  --node-name <name>       Display name for this host node\n"
    printf "  --install-docker         Automatically install official Docker CE if missing\n"
    printf "  --configure-firewall     Automatically open ports in UFW / firewalld\n"
    printf "  --local-binary <path>    Path to local pre-built tentacle binary\n"
    printf "  --version <tag>          Tentacle version to install (Default: %s)\n" "${DEFAULT_TENTACLE_VERSION}"
    printf "  --unattended, -y         Run non-interactively without user prompts\n"
    printf "  --help, -h               Show this help message and exit\n\n"
    exit 0
}

parse_args() {
    while [ $# -gt 0 ]; do
        case "$1" in
            --panel-url)
                PANEL_URL="$2"
                shift 2
                ;;
            --token)
                NODE_TOKEN="$2"
                shift 2
                ;;
            --api-port)
                API_PORT="$2"
                shift 2
                ;;
            --sftp-port)
                SFTP_PORT="$2"
                shift 2
                ;;
            --storage-path)
                STORAGE_PATH="$2"
                shift 2
                ;;
            --node-id)
                NODE_ID="$2"
                shift 2
                ;;
            --node-name)
                NODE_NAME="$2"
                shift 2
                ;;
            --install-docker)
                OPT_INSTALL_DOCKER=true
                shift
                ;;
            --configure-firewall)
                OPT_CONFIGURE_FIREWALL=true
                shift
                ;;
            --local-binary)
                LOCAL_BINARY="$2"
                shift 2
                ;;
            --version)
                TARGET_VERSION="$2"
                shift 2
                ;;
            --unattended|-y)
                UNATTENDED=true
                shift
                ;;
            --help|-h)
                show_help
                ;;
            *)
                echo "Unknown option: $1"
                echo "Run '$0 --help' for usage."
                exit 1
                ;;
        esac
    done
}

# ------------------------------------------------------------------------------
# Quick Input / Token String Parser
# ------------------------------------------------------------------------------
parse_quick_input() {
    local input="$1"

    # 1. Check for command-line style flags inside the input string
    if [[ "$input" =~ --panel-url[[:space:]=]+(\"([^\"]+)\"|\'([^\']+)\'|([^[:space:]]+)) ]]; then
        PANEL_URL="${BASH_REMATCH[2]:-${BASH_REMATCH[3]:-${BASH_REMATCH[4]}}}"
    fi

    if [[ "$input" =~ --token[[:space:]=]+(\"([^\"]+)\"|\'([^\']+)\'|([^[:space:]]+)) ]]; then
        NODE_TOKEN="${BASH_REMATCH[2]:-${BASH_REMATCH[3]:-${BASH_REMATCH[4]}}}"
    fi

    if [[ "$input" =~ --api-port[[:space:]=]+([0-9]+) ]]; then
        API_PORT="${BASH_REMATCH[1]}"
    fi

    if [[ "$input" =~ --sftp-port[[:space:]=]+([0-9]+) ]]; then
        SFTP_PORT="${BASH_REMATCH[1]}"
    fi

    if [[ "$input" =~ --storage-path[[:space:]=]+(\"([^\"]+)\"|\'([^\']+)\'|([^[:space:]]+)) ]]; then
        STORAGE_PATH="${BASH_REMATCH[2]:-${BASH_REMATCH[3]:-${BASH_REMATCH[4]}}}"
    fi

    # 2. Check for "URL TOKEN" pair
    if [ -z "$PANEL_URL" ] || [ -z "$NODE_TOKEN" ]; then
        local words=($input)
        if [ ${#words[@]} -ge 2 ]; then
            if [[ "${words[0]}" =~ ^https?:// ]]; then
                [ -z "$PANEL_URL" ] && PANEL_URL="${words[0]}"
                [ -z "$NODE_TOKEN" ] && NODE_TOKEN="${words[1]}"
            fi
        elif [ ${#words[@]} -eq 1 ]; then
            if [[ "${words[0]}" =~ ^https?:// ]]; then
                [ -z "$PANEL_URL" ] && PANEL_URL="${words[0]}"
            elif [[ "${words[0]}" =~ ^(oct_|node_|[a-f0-9]{32,}|eyJ) ]]; then
                [ -z "$NODE_TOKEN" ] && NODE_TOKEN="${words[0]}"
            fi
        fi
    fi
}

# ==============================================================================
# Pipeline Stages
# ==============================================================================

# ------------------------------------------------------------------------------
# Stage 1: Pre-flight & System Detection ([1/6])
# ------------------------------------------------------------------------------
stage_preflight() {
    # 1. Root Check
    if [ "$(id -u)" -ne 0 ]; then
        show_error_box "[1/6]" "Root privileges required" "This installer must be run as root (UID 0) or via sudo."
        exit 1
    fi

    # Ensure log directory
    mkdir -p "${LOG_DIR}"
    touch "${LOG_FILE}"
    chmod 0755 "${LOG_DIR}"

    echo "[$(date '+%Y-%m-%d %H:%M:%S')] Starting Pre-flight System Detection" >> "${LOG_FILE}"

    # 2. OS Detection
    if [ ! -f /etc/os-release ]; then
        show_error_box "[1/6]" "Missing /etc/os-release" "Unable to identify Linux distribution. /etc/os-release is required."
        exit 1
    fi

    # shellcheck disable=SC1091
    . /etc/os-release
    DISTRO_ID="${ID:-unknown}"
    local distro_like="${ID_LIKE:-}"
    DISTRO_NAME="${PRETTY_NAME:-$DISTRO_ID}"

    if [[ "$DISTRO_ID" =~ ^(debian|ubuntu|pop|linuxmint|kali|raspbian)$ ]] || [[ "$distro_like" =~ (debian|ubuntu) ]]; then
        OS_FAMILY="debian"
        PKG_MANAGER="apt-get"
    elif [[ "$DISTRO_ID" =~ ^(rhel|centos|rocky|almalinux|fedora|ol|amzn)$ ]] || [[ "$distro_like" =~ (rhel|fedora|centos) ]]; then
        OS_FAMILY="rhel"
        if command -v dnf &>/dev/null; then
            PKG_MANAGER="dnf"
        else
            PKG_MANAGER="yum"
        fi
    elif [[ "$DISTRO_ID" =~ ^(arch|manjaro|endeavouros|artix)$ ]] || [[ "$distro_like" =~ arch ]]; then
        OS_FAMILY="arch"
        PKG_MANAGER="pacman"
    else
        show_error_box "[1/6]" "Unsupported Linux distribution" "Detected '${DISTRO_NAME}'. Tentacle supports Debian/Ubuntu, RHEL/Rocky/Alma/Fedora, and Arch Linux."
        exit 1
    fi

    # 3. CPU Architecture Check
    local raw_arch
    raw_arch="$(uname -m)"
    case "$raw_arch" in
        x86_64|amd64)
            ARCH="x86_64"
            TARGET_ARCH="x86_64-unknown-linux-gnu"
            ;;
        aarch64|arm64)
            ARCH="aarch64"
            TARGET_ARCH="aarch64-unknown-linux-gnu"
            ;;
        *)
            show_error_box "[1/6]" "Unsupported CPU architecture" "Detected '${raw_arch}'. Tentacle requires an x86_64 or aarch64 host processor."
            exit 1
            ;;
    esac

    # 4. Init System Verification
    if [ ! -d /run/systemd/system ] && ! command -v systemctl &>/dev/null; then
        show_error_box "[1/6]" "Systemd not detected" "Tentacle requires systemd as the init daemon to manage background services."
        exit 1
    fi

    echo "[$(date '+%Y-%m-%d %H:%M:%S')] Detected OS: ${DISTRO_NAME} (${OS_FAMILY}), Arch: ${ARCH}, Init: systemd" >> "${LOG_FILE}"
}

# ------------------------------------------------------------------------------
# Stage 2: Kernel & Cgroups v2 Check ([2/6])
# ------------------------------------------------------------------------------
check_cgroups_v2() {
    local fs_type=""
    if [ -d /sys/fs/cgroup ]; then
        fs_type="$(stat -fc %T /sys/fs/cgroup 2>/dev/null || true)"
        if [ "$fs_type" = "cgroup2fs" ]; then
            return 0
        fi
        if grep -q "cgroup2 /sys/fs/cgroup cgroup2" /proc/mounts 2>/dev/null; then
            return 0
        fi
    fi
    return 1
}

stage_cgroups() {
    if check_cgroups_v2; then
        return 0
    fi

    # Cgroups v1 detected
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] Warning: Cgroups v1 detected on host" >> "${LOG_FILE}"

    printf "  %s  ${CLR_YELLOW}Notice: Unified Cgroups v2 hierarchy is not active.${CLR_RESET}\n" "${GLYPH_WARN}"
    printf "     Modern container memory, CPU, and swap limits require Cgroups v2.\n"

    if [ "$UNATTENDED" = false ] && [ -f /etc/default/grub ]; then
        printf "     Would you like the installer to enable Cgroups v2 in /etc/default/grub? [Y/n] "
        local answer
        read -r answer
        answer="${answer:-y}"
        if [[ "$answer" =~ ^[Yy]$ ]]; then
            enable_cgroups_grub
            return 0
        fi
    fi

    printf "     ${CLR_GRAY}To enable manually, add 'systemd.unified_cgroup_hierarchy=1' to your kernel boot parameters.${CLR_RESET}\n"
    return 0
}

enable_cgroups_grub() {
    local grub_cfg="/etc/default/grub"
    if [ ! -f "$grub_cfg" ]; then
        echo "GRUB configuration not found at $grub_cfg" >> "${LOG_FILE}"
        return 0
    fi

    if grep -q "systemd.unified_cgroup_hierarchy=1" "$grub_cfg"; then
        echo "Cgroups v2 flag already present in $grub_cfg" >> "${LOG_FILE}"
        return 0
    fi

    cp "$grub_cfg" "${grub_cfg}.bak.$(date +%Y%m%d%H%M%S)"
    if grep -q "^GRUB_CMDLINE_LINUX_DEFAULT=" "$grub_cfg"; then
        sed -i 's/^GRUB_CMDLINE_LINUX_DEFAULT="/GRUB_CMDLINE_LINUX_DEFAULT="systemd.unified_cgroup_hierarchy=1 /' "$grub_cfg"
    elif grep -q "^GRUB_CMDLINE_LINUX=" "$grub_cfg"; then
        sed -i 's/^GRUB_CMDLINE_LINUX="/GRUB_CMDLINE_LINUX="systemd.unified_cgroup_hierarchy=1 /' "$grub_cfg"
    fi

    echo "Updated $grub_cfg with systemd.unified_cgroup_hierarchy=1" >> "${LOG_FILE}"

    if command -v update-grub &>/dev/null; then
        update-grub >> "${LOG_FILE}" 2>&1 || true
    elif command -v grub2-mkconfig &>/dev/null; then
        if [ -f /boot/grub2/grub.cfg ]; then
            grub2-mkconfig -o /boot/grub2/grub.cfg >> "${LOG_FILE}" 2>&1 || true
        elif [ -f /boot/grub/grub.cfg ]; then
            grub2-mkconfig -o /boot/grub/grub.cfg >> "${LOG_FILE}" 2>&1 || true
        fi
    fi

    printf "  %s  Kernel parameters updated. A system reboot will be required for Cgroups v2.\n" "${GLYPH_INFO}"
}

# ------------------------------------------------------------------------------
