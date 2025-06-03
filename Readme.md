# OpenVPN3 GUI

A modern, user-friendly graphical interface for OpenVPN3 Linux client built with Rust and egui.

[![Build Status](https://github.com/RustNSparks/openvpn3-gui/workflows/Build%20and%20Release/badge.svg)](https://github.com/RustNSparks/openvpn3-gui/actions)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Latest Release](https://img.shields.io/github/v/release/RustNSparks/openvpn3-gui)](https://github.com/RustNSparks/openvpn3-gui/releases/latest)

## 🚀 Features

- **Easy Connection Management**: One-click connect/disconnect with visual status indicators
- **Configuration Management**: Import, view, and manage OpenVPN configuration files
- **Session Monitoring**: Real-time monitoring of active VPN sessions
- **Live Statistics**: View connection statistics, data transfer, and connection duration
- **Authentication Handling**: Secure credential input with automatic prompt detection
- **Live Logging**: Stream OpenVPN3 logs in real-time for troubleshooting
- **Error Management**: Comprehensive error handling with user-friendly messages
- **Cross-Distribution**: Works on any Linux distribution with OpenVPN3 support

## 📸 Screenshots

*Screenshots will be added soon*

<!-- Placeholder for future screenshots
![Main Interface](screenshots/main-interface.png)
![Connection Status](screenshots/connection-status.png)
![Configuration Management](screenshots/config-management.png)
-->

## 📋 Prerequisites

### Required Software
- **OpenVPN3 Linux Client**: Must be installed and properly configured
  - Ubuntu/Debian: `sudo apt install openvpn3`
  - Fedora/RHEL: `sudo dnf install openvpn3-client`
  - Arch Linux: `yay -S openvpn3-git`
  - Or build from source: [OpenVPN3 Linux](https://github.com/OpenVPN/openvpn3-linux)

### System Requirements
- Linux with GUI environment (X11 or Wayland)
- OpenVPN3 service running (`sudo systemctl enable --now openvpn3-session@.service`)
- Appropriate permissions to manage OpenVPN3 (usually requires being in `openvpn` group)

### Setup OpenVPN3 Permissions
```bash
# Add your user to the openvpn group
sudo usermod -a -G openvpn $USER

# Log out and back in, or run:
newgrp openvpn

# Verify OpenVPN3 is working
openvpn3 version
```

## 🔧 Installation

### Option 1: AppImage (Recommended)
The easiest way to run OpenVPN3 GUI on any Linux distribution:

1. Download the latest AppImage from [Releases](https://github.com/RustNSparks/openvpn3-gui/releases/latest)
2. Make it executable and run:
   ```bash
   chmod +x openvpn3-gui-x86_64.AppImage
   ./openvpn3-gui-x86_64.AppImage
   ```

### Option 2: Pre-built Binaries
Download the appropriate binary for your system:

- **Standard Linux** (most distributions): `openvpn3-gui-x86_64-unknown-linux-gnu.tar.gz`
- **Static Binary** (minimal dependencies): `openvpn3-gui-x86_64-unknown-linux-musl-static.tar.gz`

```bash
# Extract and run
tar -xzf openvpn3-gui-*.tar.gz
chmod +x openvpn3-gui
./openvpn3-gui
```

### Option 3: Build from Source
See [Building from Source](#building-from-source) section below.

## 🎯 Usage

### First Time Setup
1. Launch OpenVPN3 GUI
2. Go to the **Configurations** tab
3. Click **Import Config File** to add your `.ovpn` files
4. Switch to the **Connection** tab to connect

### Basic Operations

#### Connecting to a VPN
1. Select a configuration from the dropdown
2. Click **Connect**
3. Enter credentials if prompted
4. Monitor status in real-time

#### Managing Configurations
- **Import**: Add new `.ovpn` configuration files
- **View/Dump**: Examine configuration contents
- **Remove**: Delete configurations you no longer need

#### Monitoring Connections
- **Sessions Tab**: View all active OpenVPN3 sessions
- **Statistics Tab**: Real-time connection statistics
- **Logs Tab**: Live logging and error tracking

### Advanced Features

#### Authentication
The GUI automatically detects when authentication is required and presents a secure input dialog. Passwords are masked and handled securely.

#### Logging
- **Live VPN Logs**: Stream output from `openvpn3 log`
- **Manager Logs**: Internal application logging
- **Error Tracking**: Comprehensive error management with history

#### Settings
- Configure application behavior
- View OpenVPN3 CLI version information
- Manage error handling preferences

## 🛠️ Building from Source

### Dependencies
```bash
# Ubuntu/Debian
sudo apt update
sudo apt install -y \
    build-essential \
    pkg-config \
    libxcb-render0-dev \
    libxcb-shape0-dev \
    libxcb-xfixes0-dev \
    libxkbcommon-dev \
    libssl-dev \
    libgtk-3-dev \
    libatk1.0-dev \
    libgdk-pixbuf2.0-dev \
    libpango1.0-dev \
    libcairo2-dev

# Fedora/RHEL
sudo dnf install -y \
    gcc \
    pkg-config \
    libxcb-devel \
    libxkbcommon-devel \
    openssl-devel \
    gtk3-devel \
    atk-devel \
    gdk-pixbuf2-devel \
    pango-devel \
    cairo-devel

# Arch Linux
sudo pacman -S \
    base-devel \
    pkg-config \
    libxcb \
    libxkbcommon \
    openssl \
    gtk3 \
    atk \
    gdk-pixbuf2 \
    pango \
    cairo
```

### Install Rust
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
```

### Build and Run
```bash
# Clone the repository
git clone https://github.com/RustNSparks/openvpn3-gui.git
cd openvpn3-gui

# Build in release mode
cargo build --release

# Run
./target/release/openvpn3-gui
```

### Development Build
```bash
# For development with debug info and faster compilation
cargo run
```

## 🏗️ Architecture

### Project Structure
```
src/
├── main.rs           # Application entry point
├── app.rs            # Main application logic and UI
├── config.rs         # Configuration management
└── openvpn.rs        # OpenVPN3 integration and management
```

### Key Components

- **App Module**: Main GUI logic using egui, handles all UI interactions and state management
- **OpenVPN3 Manager**: Async task-based manager for OpenVPN3 CLI interactions
- **Config Management**: Persistent application settings and VPN configuration handling
- **Message System**: Thread-safe communication between GUI and VPN manager

### Technology Stack

- **GUI Framework**: [egui](https://github.com/emilk/egui) - Immediate mode GUI
- **Runtime**: [Tokio](https://tokio.rs/) - Async runtime for VPN operations
- **Serialization**: [Serde](https://serde.rs/) - Configuration and data serialization
- **File Dialogs**: [rfd](https://github.com/PolyMeilex/rfd) - Native file dialogs
- **Error Handling**: [anyhow](https://github.com/dtolnay/anyhow) - Flexible error handling

## 🐛 Troubleshooting

### Common Issues

#### "Command not found: openvpn3"
**Solution**: Install OpenVPN3 Linux client
```bash
# Check if installed
which openvpn3

# If not found, install for your distribution
# Ubuntu/Debian:
sudo apt install openvpn3
```

#### "Permission denied" errors
**Solution**: Ensure your user has OpenVPN3 permissions
```bash
# Add to openvpn group
sudo usermod -a -G openvpn $USER

# Restart your session or run:
newgrp openvpn
```

#### GUI doesn't start
**Solution**: Check GUI dependencies
```bash
# Verify you have a display
echo $DISPLAY

# For Wayland, you might need:
export GDK_BACKEND=wayland

# Or force X11:
export GDK_BACKEND=x11
```

#### Authentication fails repeatedly
**Solution**: 
1. Verify credentials work with OpenVPN3 CLI: `openvpn3 session-start --config your-config`
2. Check if configuration requires certificates or special authentication
3. Review logs in the Logs tab for detailed error messages

#### Configuration import fails
**Solution**:
1. Ensure the `.ovpn` file is valid
2. Check file permissions
3. Verify the configuration doesn't require additional files (certificates, keys)

### Debug Mode
Run with debug logging:
```bash
RUST_LOG=debug ./openvpn3-gui
```

### Getting Help
1. Check the [Issues](https://github.com/RustNSparks/openvpn3-gui/issues) page
2. Review OpenVPN3 Linux documentation
3. Enable debug logging and include logs when reporting issues

## 🤝 Contributing

We welcome contributions! Please see our [Contributing Guidelines](CONTRIBUTING.md) for details.

### Development Setup
1. Fork the repository
2. Create a feature branch: `git checkout -b feature-name`
3. Make your changes
4. Add tests if applicable
5. Ensure code formatting: `cargo fmt`
6. Run tests: `cargo test`
7. Submit a pull request

### Code Style
- Follow Rust standard formatting (`cargo fmt`)
- Use `cargo clippy` to catch common issues
- Write descriptive commit messages
- Add documentation for public APIs

## 📝 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## 🙏 Acknowledgments

- [OpenVPN3 Linux](https://github.com/OpenVPN/openvpn3-linux) - The underlying VPN client
- [egui](https://github.com/emilk/egui) - The immediate mode GUI framework
- [Tokio](https://tokio.rs/) - Async runtime
- All contributors and users who help improve this project

## 🔗 Related Projects

- [OpenVPN3 Linux](https://github.com/OpenVPN/openvpn3-linux) - Official OpenVPN3 Linux client
- [NetworkManager-openvpn](https://github.com/GNOME/NetworkManager-openvpn) - GNOME NetworkManager plugin
- [OpenVPN Connect](https://openvpn.net/client-connect-vpn-for-linux/) - Official OpenVPN GUI client

---

**Made with ❤️ by the OpenVPN3 GUI community**

*If you find this project useful, please consider giving it a ⭐ on GitHub!*