# """
# Makefile for Python bindings development
# """

.PHONY: help dev build test clean install

help:
	@echo "Ryx Python Bindings"
	@echo ""
	@echo "Available commands:"
	@echo "  make dev      - Build and install in development mode"
	@echo "  make build    - Build release wheel"
	@echo "  make test     - Run tests"
	@echo "  make clean    - Clean build artifacts"
	@echo "  make install  - Install package"

dev:
	maturin develop --release

build:
	maturin build --release

test:
	python test.py all

clean:
	rm -rf target/
	rm -rf Ryx.egg-info/
	rm -rf dist/
	find . -type d -name __pycache__ -exec rm -rf {} +
	find . -type f -name "*.pyc" -delete

install: build
	uv pip install target/wheels/*.whl
