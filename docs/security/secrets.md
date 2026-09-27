# Secrets

Passwords are stored only in the system Secret Service (GNOME Keyring,
visible in Seahorse) via the `secret-service` crate on the D-Bus session bus.
A `ConnectionProfile` carries a `SecretReference` like `mssql/7/password`
generated on insert; SQLite and TOML store the reference, never the secret.
At connect time the driver path fetches the password into a
`secrecy::SecretString`, so `Debug`, `Display`, and `Serialize` cannot leak
it. Secret material never reaches logs or error messages.

**Implemented:** `SecretReference` and `Credentials` domain types;
`ConnectionRepo` generates and persists references. **Pending:**
`datara-secrets` store implementation — phase 2 (task 2.1).
