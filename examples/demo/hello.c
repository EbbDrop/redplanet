#include "uart.h"

void power_down() {
    (*(volatile int*)0x100000) = 0x5555;
}

void prints(char s[]) {
    char c;
    while (c = *s++) {
        uartputc(c);
    }
}

int main() {
    uartinit();

    prints("Hello, world!\n");
    prints("Type a character: ");

    char c = 0;
    while (!(c = uartgetc()))
        ;

    prints("\nYou typed: ");
    uartputc(c);

    power_down();

    while (1);
}
