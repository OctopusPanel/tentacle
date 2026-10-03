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
    CLR_RESET=$'\033[0m'
    CLR_BOLD=$'\033[1m'
    CLR_DIM=$'\033[2m'
    CLR_CYAN=$'\033[38;2;0;240;255m'      # #00F0FF / ANSI 14
    CLR_PURPLE=$'\033[38;2;189;147;249m'  # #BD93F9 / ANSI 13
    CLR_GREEN=$'\033[38;2;80;250;123m'    # #50FA7B / ANSI 10
    CLR_YELLOW=$'\033[38;2;241;250;140m'  # #F1FA8C / ANSI 11
    CLR_RED=$'\033[38;2;255;85;85m'       # #FF5555 / ANSI 9
    CLR_GRAY=$'\033[38;2;98;114;164m'     # #6272A4 / ANSI 8
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
# Stage 3: Container Engine (Docker) Setup ([3/6])
# ------------------------------------------------------------------------------
install_docker_engine() {
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] Installing official Docker CE" >> "${LOG_FILE}"

    # Ensure curl is installed
    if ! command -v curl &>/dev/null; then
        case "$OS_FAMILY" in
            debian)
                apt-get update -qq >> "${LOG_FILE}" 2>&1
                apt-get install -y -qq curl >> "${LOG_FILE}" 2>&1
                ;;
            rhel)
                $PKG_MANAGER install -y -q curl >> "${LOG_FILE}" 2>&1
                ;;
            arch)
                pacman -Sy --noconfirm curl >> "${LOG_FILE}" 2>&1
                ;;
        esac
    fi

    # Use official Docker convenience script with silent output
    curl -fsSL https://get.docker.com | sh >> "${LOG_FILE}" 2>&1

    # Start and enable docker
    systemctl daemon-reload >> "${LOG_FILE}" 2>&1 || true
    systemctl enable --now docker >> "${LOG_FILE}" 2>&1

    # Wait for docker socket
    local attempts=0
    while [ ! -S /var/run/docker.sock ] && [ $attempts -lt 15 ]; do
        sleep 1
        attempts=$((attempts + 1))
    done

    if [ ! -S /var/run/docker.sock ]; then
        echo "Docker socket /var/run/docker.sock not available after installation" >> "${LOG_FILE}"
        return 1
    fi

    docker info >> "${LOG_FILE}" 2>&1
}

stage_docker() {
    # Check if docker is installed and operational
    if command -v docker &>/dev/null; then
        if ! systemctl is-active --quiet docker 2>/dev/null; then
            systemctl start docker >> "${LOG_FILE}" 2>&1 || true
        fi

        if docker info >> "${LOG_FILE}" 2>&1; then
            echo "[$(date '+%Y-%m-%d %H:%M:%S')] Existing Docker installation verified and running" >> "${LOG_FILE}"
            return 0
        fi
    fi

    # Docker is missing or inactive
    if [ "$OPT_INSTALL_DOCKER" = false ]; then
        if [ "$UNATTENDED" = true ]; then
            show_error_box "[3/6]" "Docker Engine is not installed" "Docker is required to manage server containers. Pass --install-docker to automate installation."
            exit 1
        else
            printf "\n  %s  Docker Engine was not detected on this system.\n" "${GLYPH_WARN}"
            printf "     Would you like to install the official Docker CE Engine now? [Y/n] "
            local answer
            read -r answer
            answer="${answer:-y}"
            if [[ ! "$answer" =~ ^[Yy]$ ]]; then
                show_error_box "[3/6]" "Docker Engine required" "Tentacle requires Docker Engine to provision and run game server containers."
                exit 1
            fi
        fi
    fi

    if ! run_step "[3/6] Installing official Docker CE Engine" install_docker_engine; then
        show_error_box "[3/6]" "Docker Engine installation failed" "Unable to install or initialize Docker daemon. Review log for details."
        exit 1
    fi
}

# ------------------------------------------------------------------------------
# Stage 4: Tentacle Binary & Storage Provisioning ([4/6])
# ------------------------------------------------------------------------------
setup_directories() {
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] Setting up directory hierarchy" >> "${LOG_FILE}"
    mkdir -p "${CONFIG_DIR}"
    chmod 0755 "${CONFIG_DIR}"

    local storage="${STORAGE_PATH:-$DEFAULT_STORAGE_PATH}"
    mkdir -p "${storage}"
    chmod 0750 "${storage}"

    mkdir -p "${DEFAULT_BACKUPS_PATH}"
    chmod 0750 "${DEFAULT_BACKUPS_PATH}"

    mkdir -p "${DEFAULT_TMP_PATH}"
    chmod 0750 "${DEFAULT_TMP_PATH}"

    mkdir -p "${LOG_DIR}"
    chmod 0755 "${LOG_DIR}"

    mkdir -p "${BINARY_DIR}"
}

provision_binary() {
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] Provisioning Tentacle binary" >> "${LOG_FILE}"

    # 1. Local binary specified via flag
    if [ -n "$LOCAL_BINARY" ]; then
        if [ -f "$LOCAL_BINARY" ]; then
            echo "Installing binary from specified local path: $LOCAL_BINARY" >> "${LOG_FILE}"
            install -m 0755 "$LOCAL_BINARY" "${BINARY_PATH}"
            return 0
        else
            echo "Specified --local-binary '$LOCAL_BINARY' not found" >> "${LOG_FILE}"
            return 1
        fi
    fi

    # 2. Local build in workspace (e.g. target/release/tentacle or target/debug/tentacle)
    local script_dir
    script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    if [ -f "${script_dir}/target/release/tentacle" ]; then
        echo "Found local release binary at ${script_dir}/target/release/tentacle" >> "${LOG_FILE}"
        install -m 0755 "${script_dir}/target/release/tentacle" "${BINARY_PATH}"
        return 0
    elif [ -f "${script_dir}/target/debug/tentacle" ]; then
        echo "Found local debug binary at ${script_dir}/target/debug/tentacle" >> "${LOG_FILE}"
        install -m 0755 "${script_dir}/target/debug/tentacle" "${BINARY_PATH}"
        return 0
    fi

    # 3. Download from GitHub Releases
    local tmp_dir
    tmp_dir="$(mktemp -d /tmp/tentacle-install.XXXXXX)"
    # shellcheck disable=SC2064
    trap "rm -rf '${tmp_dir}'" RETURN

    local tag="${TARGET_VERSION}"
    local download_url=""

    if [ "$tag" = "latest" ]; then
        download_url="https://github.com/${GITHUB_REPO}/releases/latest/download/tentacle-${TARGET_ARCH}.tar.gz"
    else
        download_url="https://github.com/${GITHUB_REPO}/releases/download/${tag}/tentacle-${TARGET_ARCH}.tar.gz"
    fi

    echo "Attempting download from: ${download_url}" >> "${LOG_FILE}"

    if curl -fsSL "${download_url}" -o "${tmp_dir}/tentacle.tar.gz" >> "${LOG_FILE}" 2>&1; then
        # Check for optional checksum file
        local sha_url="${download_url}.sha256"
        if curl -fsSL "${sha_url}" -o "${tmp_dir}/tentacle.tar.gz.sha256" >> "${LOG_FILE}" 2>&1; then
            echo "Verifying SHA256 checksum..." >> "${LOG_FILE}"
            (cd "${tmp_dir}" && sha256sum -c "tentacle.tar.gz.sha256") >> "${LOG_FILE}" 2>&1 || {
                echo "Checksum verification failed" >> "${LOG_FILE}"
                return 1
            }
        fi

        tar -xzf "${tmp_dir}/tentacle.tar.gz" -C "${tmp_dir}" >> "${LOG_FILE}" 2>&1
        if [ -f "${tmp_dir}/tentacle" ]; then
            install -m 0755 "${tmp_dir}/tentacle" "${BINARY_PATH}"
            return 0
        fi
    fi

    # 4. Fallback: Direct binary release download
    local direct_url=""
    if [ "$tag" = "latest" ]; then
        direct_url="https://github.com/${GITHUB_REPO}/releases/latest/download/tentacle-${TARGET_ARCH}"
    else
        direct_url="https://github.com/${GITHUB_REPO}/releases/download/${tag}/tentacle-${TARGET_ARCH}"
    fi

    echo "Attempting direct binary download from: ${direct_url}" >> "${LOG_FILE}"
    if curl -fsSL "${direct_url}" -o "${tmp_dir}/tentacle" >> "${LOG_FILE}" 2>&1; then
        install -m 0755 "${tmp_dir}/tentacle" "${BINARY_PATH}"
        return 0
    fi

    # 5. Fallback: Build with local Cargo if available in repository
    if [ -f "${script_dir}/Cargo.toml" ] && command -v cargo &>/dev/null; then
        echo "Building release binary via local Cargo..." >> "${LOG_FILE}"
        cargo build --release --manifest-path "${script_dir}/Cargo.toml" >> "${LOG_FILE}" 2>&1
        if [ -f "${script_dir}/target/release/tentacle" ]; then
            install -m 0755 "${script_dir}/target/release/tentacle" "${BINARY_PATH}"
            return 0
        fi
    fi

    echo "Failed to acquire Tentacle binary through release downloads or local builds." >> "${LOG_FILE}"
    return 1
}

stage_provisioning() {
    setup_directories

    if ! run_step "[4/6] Provisioning Tentacle binary & storage structure" provision_binary; then
        show_error_box "[4/6]" "Binary provisioning failed" "Could not download or install the Tentacle binary for architecture ${ARCH}. Check internet connection or specify --local-binary."
        exit 1
    fi

    # Verify execution permissions and binary integrity
    chmod 0755 "${BINARY_PATH}"
    if ! "${BINARY_PATH}" --help >> "${LOG_FILE}" 2>&1; then
        show_error_box "[4/6]" "Binary verification failed" "The installed binary at ${BINARY_PATH} failed to execute properly."
        exit 1
    fi
}

# ------------------------------------------------------------------------------
# Stage 5: Node Configuration Wizard ([5/6])
# ------------------------------------------------------------------------------
stage_config_wizard() {
    # If interactive and missing panel-url or token, offer 1-click token string paste
    if [ "$UNATTENDED" = false ] && { [ -z "$PANEL_URL" ] || [ -z "$NODE_TOKEN" ]; }; then
        printf "\n"
        printf "  %s  ${CLR_BOLD}OctopusPanel Node Configuration${CLR_RESET}\n" "${GLYPH_PROMPT}"
        printf "     ${CLR_GRAY}Paste the 1-click setup string from OctopusPanel UI, or press Enter for step-by-step setup:${CLR_RESET}\n"
        printf "     > "
        local quick_input
        read -r quick_input
        if [ -n "$quick_input" ]; then
            parse_quick_input "$quick_input"
        fi
    fi

    # Interactive Step-by-Step Prompts if parameters are still missing
    if [ "$UNATTENDED" = false ]; then
        # 1. Panel URL
        while [ -z "$PANEL_URL" ]; do
            printf "  %s  Panel URL (e.g., https://panel.example.com): " "${GLYPH_PROMPT}"
            read -r PANEL_URL
            if [ -n "$PANEL_URL" ]; then
                if [[ ! "$PANEL_URL" =~ ^https?:// ]]; then
                    printf "     ${CLR_YELLOW}Panel URL must start with http:// or https://${CLR_RESET}\n"
                    PANEL_URL=""
                fi
            fi
        done

        # 2. Node Authentication Token
        while [ -z "$NODE_TOKEN" ]; do
            printf "  %s  Node Authentication Secret / Token: " "${GLYPH_PROMPT}"
            read -r NODE_TOKEN
            if [ -z "$NODE_TOKEN" ]; then
                printf "     ${CLR_YELLOW}Authentication token cannot be empty.${CLR_RESET}\n"
            fi
        done

        # 3. API Port
        if [ -z "$API_PORT" ]; then
            printf "  %s  API Listen Port [%s]: " "${GLYPH_PROMPT}" "${DEFAULT_API_PORT}"
            read -r input_port
            API_PORT="${input_port:-$DEFAULT_API_PORT}"
        fi

        # 4. SFTP Port
        if [ -z "$SFTP_PORT" ]; then
            printf "  %s  SFTP Listen Port [%s]: " "${GLYPH_PROMPT}" "${DEFAULT_SFTP_PORT}"
            read -r input_sftp
            SFTP_PORT="${input_sftp:-$DEFAULT_SFTP_PORT}"
        fi

        # 5. Storage Path
        if [ -z "$STORAGE_PATH" ]; then
            printf "  %s  Container Storage Root [%s]: " "${GLYPH_PROMPT}" "${DEFAULT_STORAGE_PATH}"
            read -r input_storage
            STORAGE_PATH="${input_storage:-$DEFAULT_STORAGE_PATH}"
        fi
    fi

    # Apply defaults if still unset (e.g. Unattended mode with optional flags omitted)
    API_PORT="${API_PORT:-$DEFAULT_API_PORT}"
    SFTP_PORT="${SFTP_PORT:-$DEFAULT_SFTP_PORT}"
    STORAGE_PATH="${STORAGE_PATH:-$DEFAULT_STORAGE_PATH}"

    # Validation in unattended mode
    if [ -z "$PANEL_URL" ] || [ -z "$NODE_TOKEN" ]; then
        show_error_box "[5/6]" "Missing configuration parameters" "Both --panel-url and --token must be supplied in unattended mode."
        exit 1
    fi

    # Node ID and Node Name defaults
    if [ -z "$NODE_ID" ]; then
        local machine_id=""
        if [ -f /etc/machine-id ]; then
            machine_id="$(head -c 8 /etc/machine-id 2>/dev/null || true)"
        fi
        if [ -z "$machine_id" ]; then
            machine_id="$(hostname -s 2>/dev/null || echo "01")"
        fi
        NODE_ID="node-${machine_id}"
    fi

    if [ -z "$NODE_NAME" ]; then
        local host_display
        host_display="$(hostname -f 2>/dev/null || hostname -s 2>/dev/null || echo "node")"
        NODE_NAME="Tentacle Node (${host_display})"
    fi

    # Backup existing configuration if present
    if [ -f "${CONFIG_FILE}" ]; then
        local backup_path="${CONFIG_FILE}.bak.$(date +%Y%m%d%H%M%S)"
        echo "Backing up existing config to ${backup_path}" >> "${LOG_FILE}"
        cp "${CONFIG_FILE}" "${backup_path}"
    fi

    # Generate tentacle.yaml
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] Generating configuration at ${CONFIG_FILE}" >> "${LOG_FILE}"
    cat <<EOF > "${CONFIG_FILE}"
# ==============================================================================
#  Tentacle Node Daemon Configuration
#  Generated by Tentacle Installer ${INSTALLER_VERSION} on $(date -u '+%Y-%m-%dT%H:%M:%SZ')
# ==============================================================================

node:
  id: "${NODE_ID}"
  name: "${NODE_NAME}"
  listen_host: "0.0.0.0"
  listen_port: ${API_PORT}
  base_url: "${PANEL_URL}"

auth:
  panel_secret: "${NODE_TOKEN}"
  token_expiry_secs: 3600
  jwt_audience: "tentacle-node"
  jwt_issuer: "octopus-panel"

docker:
  socket_path: "/var/run/docker.sock"
  network: "bridge"
  connection_timeout_secs: 30

storage:
  volumes_path: "${STORAGE_PATH}"
  backups_path: "${DEFAULT_BACKUPS_PATH}"
  temp_path: "${DEFAULT_TMP_PATH}"

sftp:
  enabled: true
  listen_host: "0.0.0.0"
  listen_port: ${SFTP_PORT}
  host_key_path: "${CONFIG_DIR}/host_key"

resources:
  default_cpu_limit: null
  default_memory_limit_mb: null
  metrics_poll_interval_secs: 2

system:
  log_level: "info"
EOF

    chmod 0600 "${CONFIG_FILE}"
    printf "  %s  %s\n" "${GLYPH_SUCCESS}" "[5/6] Node configuration generated securely (${CONFIG_FILE})"
}

# ------------------------------------------------------------------------------
# Stage 6: Firewall & Systemd Service ([6/6])
# ------------------------------------------------------------------------------
configure_firewall() {
    local configured=false

    # 1. UFW Check
    if command -v ufw &>/dev/null && ufw status 2>/dev/null | grep -qw "active"; then
        local should_open=false
        if [ "$OPT_CONFIGURE_FIREWALL" = true ]; then
            should_open=true
        elif [ "$UNATTENDED" = false ]; then
            printf "  %s  Active UFW firewall detected. Open ports %s (API) and %s (SFTP)? [Y/n] " "${GLYPH_PROMPT}" "${API_PORT}" "${SFTP_PORT}"
            local ans
            read -r ans
            ans="${ans:-y}"
            [[ "$ans" =~ ^[Yy]$ ]] && should_open=true
        fi

        if [ "$should_open" = true ]; then
            echo "Opening UFW ports: ${API_PORT}/tcp, ${SFTP_PORT}/tcp" >> "${LOG_FILE}"
            ufw allow "${API_PORT}/tcp" comment "Octopus Tentacle API" >> "${LOG_FILE}" 2>&1 || true
            ufw allow "${SFTP_PORT}/tcp" comment "Octopus Tentacle SFTP" >> "${LOG_FILE}" 2>&1 || true
            configured=true
        fi
    fi

    # 2. Firewalld Check
    if command -v firewall-cmd &>/dev/null && firewall-cmd --state 2>/dev/null | grep -qw "running"; then
        local should_open=false
        if [ "$OPT_CONFIGURE_FIREWALL" = true ]; then
            should_open=true
        elif [ "$UNATTENDED" = false ] && [ "$configured" = false ]; then
            printf "  %s  Active firewalld detected. Open ports %s (API) and %s (SFTP)? [Y/n] " "${GLYPH_PROMPT}" "${API_PORT}" "${SFTP_PORT}"
            local ans
            read -r ans
            ans="${ans:-y}"
            [[ "$ans" =~ ^[Yy]$ ]] && should_open=true
        fi

        if [ "$should_open" = true ]; then
            echo "Opening firewalld ports: ${API_PORT}/tcp, ${SFTP_PORT}/tcp" >> "${LOG_FILE}"
            firewall-cmd --permanent --add-port="${API_PORT}/tcp" >> "${LOG_FILE}" 2>&1 || true
            firewall-cmd --permanent --add-port="${SFTP_PORT}/tcp" >> "${LOG_FILE}" 2>&1 || true
            firewall-cmd --reload >> "${LOG_FILE}" 2>&1 || true
            configured=true
        fi
    fi

    return 0
}

setup_systemd_service() {
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] Installing systemd unit file at ${SYSTEMD_SERVICE_FILE}" >> "${LOG_FILE}"

    cat <<EOF > "${SYSTEMD_SERVICE_FILE}"
[Unit]
Description=Tentacle - High-Performance Node Daemon for OctopusPanel
Documentation=https://github.com/${GITHUB_REPO}
After=network.target docker.service
Wants=docker.service

[Service]
Type=simple
User=root
WorkingDirectory=${CONFIG_DIR}
ExecStart=${BINARY_PATH} --config ${CONFIG_FILE}
Restart=always
RestartSec=5
LimitNOFILE=65536
StandardOutput=journal
StandardError=journal
SyslogIdentifier=tentacle

[Install]
WantedBy=multi-user.target
EOF

    chmod 0644 "${SYSTEMD_SERVICE_FILE}"
    systemctl daemon-reload >> "${LOG_FILE}" 2>&1
    systemctl enable --now tentacle >> "${LOG_FILE}" 2>&1
}

verify_daemon_health() {
    local attempts=0
    local max_attempts=15
    local health_url="http://127.0.0.1:${API_PORT}/api/system/health"
    local fallback_url="http://127.0.0.1:${API_PORT}/health"

    echo "[$(date '+%Y-%m-%d %H:%M:%S')] Polling daemon health endpoint at ${health_url}" >> "${LOG_FILE}"

    while [ $attempts -lt $max_attempts ]; do
        if curl -fsSL -m 2 "${health_url}" >> "${LOG_FILE}" 2>&1 || curl -fsSL -m 2 "${fallback_url}" >> "${LOG_FILE}" 2>&1; then
            echo "[$(date '+%Y-%m-%d %H:%M:%S')] Health check passed successfully" >> "${LOG_FILE}"
            return 0
        fi
        sleep 1
        attempts=$((attempts + 1))
    done

    echo "Health check polling timed out after ${max_attempts} attempts" >> "${LOG_FILE}"
    journalctl -u tentacle -n 15 --no-pager >> "${LOG_FILE}" 2>&1 || true
    return 1
}

stage_systemd_and_firewall() {
    configure_firewall

    if ! run_step "[6/6] Configuring systemd service & launching daemon" setup_systemd_service; then
        show_error_box "[6/6]" "Failed to register or start systemd service" "systemctl enable --now tentacle failed. Check journalctl -u tentacle."
        exit 1
    fi

    if ! run_step "[6/6] Verifying local daemon health check" verify_daemon_health; then
        show_error_box "[6/6]" "Health check failed" "Tentacle daemon did not respond at http://127.0.0.1:${API_PORT}/api/system/health within 15 seconds."
        exit 1
    fi
}

# ==============================================================================
# Main Orchestrator
# ==============================================================================
main() {
    parse_args "$@"

    # Stage 1: Pre-flight & System Detection
    stage_preflight

    # Display stylized ASCII header after pre-flight resolves version and arch
    render_header "${INSTALLER_VERSION}" "${ARCH}"
    printf "  %s  %s\n" "${GLYPH_SUCCESS}" "[1/6] Pre-flight system detection passed (${DISTRO_NAME}, ${ARCH})"

    # Stage 2: Kernel & Cgroups v2 Check
    stage_cgroups
    printf "  %s  %s\n" "${GLYPH_SUCCESS}" "[2/6] Kernel & Cgroups configuration verified"

    # Stage 3: Container Engine (Docker) Setup
    stage_docker
    printf "  %s  %s\n" "${GLYPH_SUCCESS}" "[3/6] Container Engine (Docker) active and operational"

    # Stage 4: Tentacle Binary & Storage Provisioning
    stage_provisioning

    # Stage 5: Node Configuration Wizard
    stage_config_wizard

    # Stage 6: Firewall & Systemd Service
    stage_systemd_and_firewall

    # Render success banner
    show_success_box "${API_PORT}" "${SFTP_PORT}" "${CONFIG_FILE}"
}

main "$@"
