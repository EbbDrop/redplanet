run:
	riscof run --config=config.ini --suite=riscv-arch-test/riscv-test-suite --env=riscv-arch-test/riscv-test-suite/env

build-tests:
	riscof --verbose info arch-test --clone
	riscof validateyaml --config=config.ini
	riscof testlist --config=config.ini --suite=riscv-arch-test/riscv-test-suite --env=riscv-arch-test/riscv-test-suite/env
