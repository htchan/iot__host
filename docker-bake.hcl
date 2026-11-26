variable "DATE" {
  default = "${formatdate("YYYY.MM.DD",timestamp())}"
}

variable "HASH" { default = "TESTING" }

variable "BAKE_CI" { default = "false" }

variable "BRANCH" { default = "" }
variable "IMAGE_TAG" { default = "${equal(BRANCH,"main") ? "latest" : "beta"}" }

group "default" {
  targets = ["backend"]
}

target "backend" {
  context = "."
  dockerfile = "./Dockerfile"
  cache-from = equal(BAKE_CI, true) ? [
    "type=gha",
    "type=local,src=/tmp/.buildx-cache"
  ] : []
  cache-to = equal(BAKE_CI, true) ? [
    "type=gha,mode=max",
    "type=local,dest=/tmp/.buildx-cache-new,mode=max"
  ] : []

  labels = {
    "org.opencontainers.image.source" = "https://github.com/htchan/iot__host"
  }

  attest = [
    "type=provenance,disabled=true"
  ]

  tags = [
    "ghcr.io/htchan/iot__host:v${DATE}-${substr(HASH,0,7)}",
    "ghcr.io/htchan/iot__host:${IMAGE_TAG}"
  ]
  platforms = equal(BAKE_CI, "true") ? ["linux/amd64","linux/arm64"] : []
  output     = [equal(BAKE_CI, "true") ? "type=registry": "type=docker"]
}
