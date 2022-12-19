help:		## show help messages
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) \
	| sort \
	| awk 'BEGIN {FS = ":[ \t].*?## "}; {print $$1 "|" $$2 }' \
	| tr ":" "|" \
	| awk 'BEGIN {FS = "|"}; {printf "\033[32m%-16s\033[0m: \033[36m%-21s\033[0m %s\n", $$1, $$2, $$3}'

check:	## quick compiler check
	cargo check

lint:		## quick linter (clippy) check
	cargo clippy --fix

format:	## code formatter
	cargo fmt

doc:		## generate docs
	cargo doc

build-debug:				## debug build
	cargo build

build-release:			## release build
	cargo build --release

build-container:		## build container (via docker)
	docker build . \
		--network host \
		--build-arg COMMIT_SHA=$(shell git log -1 --format="%H") \
		-t webrtc-sfu

run:								## run native instance (needs Redis and NATS)
	env RUST_LOG=debug,actix_web=debug,webrtc_mdns=error,webrtc_srtp=debug \
		cargo run -- \
		--debug \
		--cert-file certs/cert.pem \
		--key-file certs/key.pem

run-container:		## run container (needs Redis and NATS)
	docker run \
		--network host \
		--env RUST_LOG=debug \
		-it webrtc-sfu

run-compose:				## run docker-compose
	docker-compose up

stop-compose:				## stop docker-compose
	docker-compose down

gen-certs:					## generate debug certs
	@echo "=> generating debug certs in 'certs' folder"
	# https://github.com/est31/rcgen
	rcgen
	openssl x509 -in certs/cert.pem -text -noout


_prepare:
	make check
	make lint
	make format
	make doc
	make build-debug
	make build-container
