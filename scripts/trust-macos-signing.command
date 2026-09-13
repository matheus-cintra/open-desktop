#!/bin/bash
set -euo pipefail
printf '%s\n' 'Open Desktop: autorizar o certificado local apenas para assinatura de codigo.'
/usr/bin/security add-trusted-cert -r trustRoot -p codeSign "$HOME/Library/Application Support/Open Desktop/signing/development.der"
printf '%s\n' 'Certificado autorizado. Esta janela pode ser fechada.'
