.PHONY: build test crdgen docker-build install-crds fmt lint clean run

CARGO ?= cargo
DOCKER ?= docker
KUBECTL ?= kubectl
IMAGE_NAME ?= ghcr.io/osodevops/kafka-backup-operator
IMAGE_TAG ?= latest
RUST_LOG ?= info

build:
	$(CARGO) build --release

test:
	$(CARGO) test

test-integration:
	$(CARGO) test --test '*' -- --ignored

fmt:
	$(CARGO) fmt --all

fmt-check:
	$(CARGO) fmt --all -- --check

lint:
	$(CARGO) clippy --all-targets --all-features -- -D warnings

crdgen:
	$(CARGO) run --bin crdgen
	@echo "CRDs generated in deploy/crds/"

docker-build:
	$(DOCKER) build -t $(IMAGE_NAME):$(IMAGE_TAG) .

docker-push: docker-build
	$(DOCKER) push $(IMAGE_NAME):$(IMAGE_TAG)

install-crds:
	$(KUBECTL) apply -f deploy/crds/

uninstall-crds:
	$(KUBECTL) delete -f deploy/crds/ --ignore-not-found

run:
	RUST_LOG=$(RUST_LOG) $(CARGO) run --bin kafka-backup-operator

clean:
	$(CARGO) clean

helm-template:
	helm template kafka-backup-operator deploy/helm/kafka-backup-operator

helm-install:
	helm install kafka-backup-operator deploy/helm/kafka-backup-operator

helm-uninstall:
	helm uninstall kafka-backup-operator
