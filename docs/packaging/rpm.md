# RPM packaging

`packaging/rpm/datara.spec` builds the release binary with cargo in `%build`
and installs to standard Fedora paths: `/usr/bin/datara`, the `.desktop`
file, AppStream metainfo, and the SVG icon under `hicolor`. Build requires
`rust`, `cargo`, and `fontconfig-devel`; the bundled-sqlite build keeps
runtime deps minimal.

Target: `rpmbuild -ba packaging/rpm/datara.spec` on Fedora produces an
installable `datara` RPM.

**Implemented:** `packaging/rpm/datara.spec` + `make rpm` target; built
datara-0.1.0 RPMs under `packaging/rpm/_build/` on Fedora 44 (task 8.2).
