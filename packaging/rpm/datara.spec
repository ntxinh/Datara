Name:           datara
Version:        0.1.0
Release:        1%{?dist}
Summary:        Native MSSQL database client

License:        GPL-3.0-only
URL:            https://github.com/ntxinh/Datara
# Tarball produced by `make rpm` via:
#   git archive --format=tar.gz --prefix=datara-VERSION/ -o datara-VERSION.tar.gz HEAD
# Crates are fetched by cargo during the build (requires network; Fedora
# koji builds would need a vendored tarball instead).
Source0:        %{name}-%{version}.tar.gz

BuildRequires:  rust >= 1.98, cargo, gcc-c++, pkgconf-pkg-config
BuildRequires:  pkgconfig(fontconfig), pkgconfig(freetype2)
BuildRequires:  desktop-file-utils, libappstream-glib
Requires:       hicolor-icon-theme
# winit dlopens the Wayland client stack and femtovg dlopens libEGL; rpm's
# dep generator can't see them. X11 fallback libs (libX11, libXcursor,
# libxcb, libxkbcommon-x11) are dlopen-optional — not required.
Requires:       libwayland-client libwayland-egl libwayland-cursor
Requires:       libxkbcommon mesa-libEGL

%description
Datara is a native Rust + Slint MSSQL database client for Linux.
It provides a connection manager, a SQL editor, a data grid for
browsing and editing results, and an MCP server so AI assistants
can query your databases.

%prep
%autosetup -n %{name}-%{version}

%build
cargo build --release --locked

%install
install -Dm755 target/release/datara %{buildroot}%{_bindir}/datara
install -Dm644 packaging/share/io.github.ntxinh.Datara.desktop \
    %{buildroot}%{_datadir}/applications/io.github.ntxinh.Datara.desktop
install -Dm644 packaging/share/io.github.ntxinh.Datara.metainfo.xml \
    %{buildroot}%{_datadir}/metainfo/io.github.ntxinh.Datara.metainfo.xml
install -Dm644 assets/icons/datara.svg \
    %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/io.github.ntxinh.Datara.svg
for size in 32 48 64 128 256 512; do
    install -Dm644 assets/icons/datara-$size.png \
        %{buildroot}%{_datadir}/icons/hicolor/${size}x${size}/apps/io.github.ntxinh.Datara.png
done

%check
desktop-file-validate %{buildroot}%{_datadir}/applications/io.github.ntxinh.Datara.desktop
appstream-util validate-relax --nonet %{buildroot}%{_datadir}/metainfo/io.github.ntxinh.Datara.metainfo.xml

%files
%license LICENSE
%{_bindir}/datara
%{_datadir}/applications/io.github.ntxinh.Datara.desktop
%{_datadir}/metainfo/io.github.ntxinh.Datara.metainfo.xml
%{_datadir}/icons/hicolor/scalable/apps/io.github.ntxinh.Datara.svg
%{_datadir}/icons/hicolor/*/apps/io.github.ntxinh.Datara.png

%changelog
* Mon Sep 28 2026 Datara Developers <packaging@github.invalid> - 0.1.0-1.fc44
- Initial RPM release
