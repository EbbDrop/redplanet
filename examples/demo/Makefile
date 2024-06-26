LD = riscv32-unknown-elf-ld
CC = riscv32-unknown-elf-gcc
GDB = riscv32-unknown-elf-gdb

SRC_S = $(wildcard *.S)
OBJ_S = $(SRC_S:.S=.o)

SRC_C = $(wildcard *.c)
OBJ_C = $(SRC_C:.c=.o)

ELF = hello.elf

.PHONY : build
build : $(ELF)

%.o: %.c
	$(CC) -march=rv32i -mabi=ilp32 -nostdlib -static -O2 -ggdb -c hello.c -o hello.o

%.o: %.S
	$(CC) -march=rv32i_zicsr -mabi=ilp32 -nostdlib -static -ggdb -c boot.S -o boot.o

$(ELF) : $(OBJ_S) $(OBJ_C)
	$(LD) -T linker.ld -static hello.o boot.o -o hello.elf

################################################################################
# Actions
################################################################################

.PHONY : run
run : hello.elf
	cargo run --bin red-planet-cli --release -- -e hello.elf

.PHONY : debug
debug : hello.elf
	cargo run --bin red-planet-cli --release -- -e hello.elf -g 1234

.PHONY : gdb
gdb : hello.elf
	$(GDB) -q -ex 'target remote :1234' -ex 'b *_start' -ex 'b *main' hello.elf

.PHONY : clean
clean :
	find . \( -name '*.o' -or -name '*.d' -or -name '*.elf' \
	           -or -name '*.so' -or -name '*.bin' -or -name '*.img' \
	       \) -delete
