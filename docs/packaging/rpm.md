# RPM packaging

`packaging/rpm/datara.spec` builds the release binary with cargo in `%build`
and installs to standard Fedora paths: `/usr/bin/datara`, the `.desktop`
file, AppStream metainfo, and the SVG icon under `hicolor`. Build requires
`rust`, `cargo`, and `fontconfig-devel`; the bundled-sqlite build keeps
runtime deps minimal.

Target: `rpmbuild -ba packaging/rpm/datara.spec` on Fedora produces an
installable `datara` RPM.

**Implemented:** nothing — spec file pending. **Pending:** phase 8
(task 8.2).
