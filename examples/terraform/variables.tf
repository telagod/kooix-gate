variable "image" {
  type        = string
  description = "Kooix Gate image tag"
  default     = "ghcr.io/telagod/kooix-gate:latest"
}

variable "master_key_base64" {
  type        = string
  description = "KOOIX_MASTER_KEY from `kgctl key master`"
  sensitive   = true
}

variable "jwt_secret_base64" {
  type        = string
  description = "KOOIX_JWT_SECRET from `kgctl key jwt`"
  sensitive   = true
}

variable "jwt_previous_secrets_base64" {
  type        = string
  description = "Optional KOOIX_JWT_PREVIOUS_SECRETS for planned JWT rotation; comma-separated old `kgctl key jwt` outputs"
  default     = ""
  sensitive   = true
}

variable "public_url" {
  type        = string
  default     = "http://localhost:8000"
}
