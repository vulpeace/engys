#!/usr/bin/env python3
"""Create a durable Android release identity without exposing passwords in argv."""
import argparse
import os
import json
from pathlib import Path
import secrets
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    default = Path(os.environ.get('XDG_DATA_HOME', Path.home() / '.local/share')) / 'engys/signing'
    parser.add_argument('--directory', type=Path, default=default)
    args = parser.parse_args()
    directory = args.directory.expanduser().resolve()
    keystore = directory / 'release.p12'
    properties = directory / 'release.properties'
    if keystore.exists() or properties.exists():
        parser.error(f'Release signing already exists in {directory}; refusing to replace it.')
    os.umask(0o077)
    directory.mkdir(parents=True, exist_ok=True, mode=0o700)
    password = secrets.token_urlsafe(48)
    environment = os.environ.copy()
    environment['MIHOMO_SIGNING_PASSWORD'] = password
    subprocess.run([
        'keytool', '-genkeypair', '-keystore', str(keystore), '-storetype', 'PKCS12',
        '-alias', 'engys', '-keyalg', 'RSA', '-keysize', '4096', '-validity', '10000',
        '-dname', 'CN=engys, O=vulpeace', '-storepass:env', 'MIHOMO_SIGNING_PASSWORD',
        '-keypass:env', 'MIHOMO_SIGNING_PASSWORD', '-noprompt',
    ], env=environment, check=True)
    keystore.chmod(0o600)
    escaped_path = json.dumps(str(keystore), ensure_ascii=True)[1:-1]
    with properties.open('x', encoding='ascii') as output:
        output.write(f'storeFile={escaped_path}\nstorePassword={password}\nkeyAlias=engys\nkeyPassword={password}\n')
    properties.chmod(0o600)
    print(f'Keystore: {keystore}')
    print(f'Signing configuration: {properties}')
    print('Back up both files securely. Future updates must use this same signing identity.')


if __name__ == '__main__':
    main()
