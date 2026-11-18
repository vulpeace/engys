export const PROFILE_TEMPLATE = `# Minimal mihomo profile — see Meta-Docs/docs/config for every option.
proxies:
  - name: example-ss
    type: ss
    server: example.com
    port: 8388
    cipher: aes-128-gcm
    password: change-me

proxy-groups:
  - name: Proxy
    type: select
    proxies: [example-ss, DIRECT]

rules:
  - MATCH,Proxy
`;
