# Terraform example

最小单机示例：创建 Docker network、Postgres、Redis 与 Kooix Gate container。适合本地 / demo，不替代生产 IaC。

```bash
cd examples/terraform
terraform init
terraform apply \
  -var='image=ghcr.io/telagod/kooix-gate:latest' \
  -var='master_key_base64=<kgctl key master output>' \
  -var='jwt_secret_base64=<kgctl key jwt output>'
```

部署后：

```bash
export KOOIX_PUBLIC_URL=http://localhost:8000
kgctl doctor --json
```
