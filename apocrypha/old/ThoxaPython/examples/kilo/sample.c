/* kilo sample: this block comment spans
   two lines to exercise the cross-row
   openComment fold. */
#include <stdio.h>

int main(int argc, char **argv) {
    int count = 42;                 // a decimal number and a line comment
    char *name = "world";
    for (int i = 0; i < count; i++) {
        printf("hello %s\n", name);
    }
    return 0;
}
