.FORCE:

build: .FORCE
	docker buildx bake backend -f docker-bake.hcl