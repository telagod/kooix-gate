provider "docker" {}

resource "docker_network" "kooix" {
  name = "kooix-gate-example"
}

resource "docker_image" "postgres" {
  name = "postgres:17-alpine"
}

resource "docker_image" "redis" {
  name = "redis:7-alpine"
}

resource "docker_image" "gate" {
  name = var.image
}

resource "docker_container" "postgres" {
  name  = "kooix-postgres"
  image = docker_image.postgres.image_id
  env = [
    "POSTGRES_DB=gate",
    "POSTGRES_USER=gate",
    "POSTGRES_PASSWORD=gate_dev"
  ]
  networks_advanced { name = docker_network.kooix.name }
}

resource "docker_container" "redis" {
  name  = "kooix-redis"
  image = docker_image.redis.image_id
  networks_advanced { name = docker_network.kooix.name }
}

resource "docker_container" "gate" {
  name  = "kooix-gate"
  image = docker_image.gate.image_id
  env = [
    "KOOIX_LISTEN_ADDR=0.0.0.0:8000",
    "KOOIX_PUBLIC_URL=${var.public_url}",
    "KOOIX_DATABASE_URL=postgres://gate:gate_dev@kooix-postgres:5432/gate",
    "KOOIX_REDIS_URL=redis://kooix-redis:6379/0",
    "KOOIX_MASTER_KEY=${var.master_key_base64}",
    "KOOIX_JWT_SECRET=${var.jwt_secret_base64}",
    "KOOIX_JWT_PREVIOUS_SECRETS=${var.jwt_previous_secrets_base64}"
  ]
  ports {
    internal = 8000
    external = 8000
  }
  networks_advanced { name = docker_network.kooix.name }
  depends_on = [docker_container.postgres, docker_container.redis]
}
