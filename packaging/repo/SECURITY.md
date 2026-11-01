# Signing key management

The apt repository is signed by the OpenPGP key
**IBDE apt repository \<apt@ibde.dev\>**
`E371E0FD 9E283ABB 72ACF074 336823BE A4EFF2AE` (RSA 3072, usage: sign)

- Public key: [`public-key.asc`](public-key.asc) (also served at the repo root)
- Private key: host GnuPG keyring of the build machine; passphrase-less
  **on purpose** so CI can sign. It is a dedicated key used for nothing
  else — compromise scope is limited to this repository.
- Revocation certificate: kept OUTSIDE this repo at
  `/apps/ibde-signing/revocation.asc` (chmod 600) with a backup copy in
  the owner's password manager. If the build machine is compromised,
  revoke with `gpg --import revocation.asc && gpg --keyserver ... --send-keys <FPR>`
  and re-issue a new key + repo.

Users install the key with:
```
curl -fsSL <repo-url>/public-key.asc | sudo tee /etc/apt/trusted.gpg.d/ibde.asc
echo "deb <repo-url> stable main" | sudo tee /etc/apt/sources.list.d/ibde.list
sudo apt update && sudo apt install ibde-shell
```
