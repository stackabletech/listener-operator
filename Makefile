# =============
# This file is automatically generated from the templates in stackabletech/operator-templating
# DO NOT MANUALLY EDIT THIS FILE
# =============

# This script requires https://github.com/mikefarah/yq (not to be confused with https://github.com/kislyuk/yq)
# It is available from Nixpkgs as `yq-go` (`nix shell nixpkgs#yq-go`)
# This script also requires `jq` https://stedolan.github.io/jq/

.PHONY: build publish

TAG    := $(shell git rev-parse --short HEAD)
OPERATOR_NAME := listener-operator
VERSION := $(shell cargo metadata --format-version 1 | jq -r '.packages[] | select(.name=="stackable-${OPERATOR_NAME}") | .version')

ORGANIZATION := stackable
DOCKER_REPO := docker.stackable.tech
OCI_REGISTRY_HOSTNAME := broadminded-goldstine.container-registry.com
OCI_REGISTRY_PROJECT_IMAGES := ${ORGANIZATION}
OCI_REGISTRY_PROJECT_CHARTS := ${OCI_REGISTRY_PROJECT_IMAGES}
# this will be overwritten by an environmental variable if called from the github action
HELM_REPO := https://repo.stackable.tech/repository/helm-dev
HELM_CHART_NAME := ${OPERATOR_NAME}
HELM_CHART_ARTIFACT := target/helm/${HELM_CHART_NAME}-${VERSION}.tgz

SHELL=/usr/bin/env bash -euo pipefail

render-readme:
	scripts/render_readme.sh

## Docker related targets
docker-build:
	docker build --force-rm --build-arg VERSION=${VERSION} -t '${OCI_REGISTRY_HOSTNAME}/${OCI_REGISTRY_PROJECT_IMAGES}/${OPERATOR_NAME}:${VERSION}' -f docker/Dockerfile .

docker-publish:
	# we need to use "value" here to prevent the variable from being recursively expanded by make (username contains a dollar sign)
	docker login --username '${value OCI_REGISTRY_USERNAME}' --password '${OCI_REGISTRY_PASSWORD}' '${OCI_REGISTRY_HOSTNAME}'
	docker push --all-tags '${OCI_REGISTRY_HOSTNAME}/${ORGANIZATION}/${OPERATOR_NAME}'
	REPO_ARTIFACT_BY_DIGEST=$$(docker inspect --format='{{range .RepoDigests}}{{ . }}{{end}}' '${OCI_REGISTRY_HOSTNAME}/${ORGANIZATION}/${OPERATOR_NAME}:${VERSION}' | grep -E '^${OCI_REGISTRY_HOSTNAME}/${ORGANIZATION}/${OPERATOR_NAME}@sha256:[0-9a-f]{64}$$' | head -n1);\
	if [ -z "$$REPO_ARTIFACT_BY_DIGEST" ]; then\
		echo 'Could not find repo digest for container image: ${OCI_REGISTRY_HOSTNAME}/${ORGANIZATION}/${OPERATOR_NAME}:${VERSION}';\
		exit 1;\
	fi;\
	cosign sign -y $$REPO_ARTIFACT_BY_DIGEST

# TODO remove if not used/needed
docker: docker-build docker-publish

print-docker-tag:
	@echo '${DOCKER_REPO}/${ORGANIZATION}/${OPERATOR_NAME}:${VERSION}'

helm-publish:
	# we need to use "value" here to prevent the variable from being recursively expanded by make (username contains a dollar sign)
	helm registry login --username '${value OCI_REGISTRY_USERNAME}' --password '${OCI_REGISTRY_PASSWORD}' '${OCI_REGISTRY_HOSTNAME}'
	REPO_ARTIFACT_BY_DIGEST=$$(helm push ${HELM_CHART_ARTIFACT} oci://${OCI_REGISTRY_HOSTNAME}/${OCI_REGISTRY_PROJECT_CHARTS} 2>&1 | awk '/^Digest: sha256:[0-9a-f]{64}$$/ { print $$2 }');\
	if [ -z "$$REPO_ARTIFACT_BY_DIGEST" ]; then\
		echo 'Could not find repo digest for helm chart: ${HELM_CHART_NAME}';\
		exit 1;\
	fi;\
	cosign sign -y ${OCI_REGISTRY_HOSTNAME}/${OCI_REGISTRY_PROJECT_CHARTS}/${HELM_CHART_NAME}:@$$REPO_ARTIFACT_BY_DIGEST

helm-package:
	mkdir -p target/helm && helm package --destination target/helm deploy/helm/${OPERATOR_NAME}

## Chart related targets
compile-chart: version crds config

chart-clean:
	rm -rf "deploy/helm/${OPERATOR_NAME}/configs"
	rm -rf "deploy/helm/${OPERATOR_NAME}/crds"

version:
	cat "deploy/helm/${OPERATOR_NAME}/Chart.yaml" | yq ".version = \"${VERSION}\" | .appVersion = \"${VERSION}\"" > "deploy/helm/${OPERATOR_NAME}/Chart.yaml.new"
	mv "deploy/helm/${OPERATOR_NAME}/Chart.yaml.new" "deploy/helm/${OPERATOR_NAME}/Chart.yaml"

config:
	if [ -d "deploy/config-spec/" ]; then\
		mkdir -p "deploy/helm/${OPERATOR_NAME}/configs";\
		cp -r deploy/config-spec/* "deploy/helm/${OPERATOR_NAME}/configs";\
	fi

crds:
	mkdir -p deploy/helm/"${OPERATOR_NAME}"/crds
	cargo run --bin stackable-"${OPERATOR_NAME}" -- crd | yq eval '.metadata.annotations["helm.sh/resource-policy"]="keep"' - > "deploy/helm/${OPERATOR_NAME}/crds/crds.yaml"

chart-lint: compile-chart
	docker run -it -v $(shell pwd):/build/helm-charts -w /build/helm-charts quay.io/helmpack/chart-testing:v3.5.0  ct lint --config deploy/helm/ct.yaml

clean: chart-clean
	cargo clean
	docker rmi --force "${DOCKER_REPO}/${ORGANIZATION}/${OPERATOR_NAME}:${VERSION}"

regenerate-charts: chart-clean compile-chart

build: regenerate-charts helm-package docker-build

publish: build docker-publish helm-publish

run-dev:
	kubectl apply -f deploy/stackable-operators-ns.yaml
	nix run -f. tilt -- up --port 5439 --namespace stackable-operators
