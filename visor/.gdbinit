define fn
si
x/20i $pc - 12
end

define loadkern
symbol-file
add-symbol-file ../kern/build/kernel.elf 0x80000
end

set history remove-duplicates 99999
set history save on
set architecture aarch64
target remote localhost:1234
add-symbol-file build/kernel.elf 0x80000

