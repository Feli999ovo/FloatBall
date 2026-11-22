"""Find unresolved Windows import-table entries in the test executable."""
import ctypes
from pathlib import Path
import sys

import pefile

kernel = ctypes.WinDLL('kernel32', use_last_error=True)
load = kernel.LoadLibraryW
load.argtypes = [ctypes.c_wchar_p]
load.restype = ctypes.c_void_p
get_proc = kernel.GetProcAddress
get_proc.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
get_proc.restype = ctypes.c_void_p
for file in Path(sys.argv[1]).glob('**/floatball_lib-*.exe'):
    print('Inspecting', file, flush=True)
    image = pefile.PE(str(file))
    for entry in image.DIRECTORY_ENTRY_IMPORT:
        dll_name = entry.dll.decode('ascii')
        library = load(dll_name)
        if not library:
            print(f'Unable to load {dll_name}: Win32 error {ctypes.get_last_error()}')
            continue
        for symbol in entry.imports:
            name = ctypes.c_char_p(symbol.name) if symbol.name else symbol.ordinal
            if not get_proc(library, ctypes.cast(name, ctypes.c_void_p) if symbol.name else name):
                print(f'Unresolved import {dll_name}!{symbol.name or symbol.ordinal}')
