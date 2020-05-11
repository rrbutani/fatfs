#pragma once

#include <stdint.h>
#include <stdbool.h>

// Assuming char has the same ABI as a uint8_t...

extern void eFile_Init();

extern void eFile_Mount(
    uint8_t drive_num,
    uint64_t size_in_sectors
);

extern bool eFile_NewFile(
    char const* path,
    uint16_t len
);

extern bool eFile_NewDir(
    char const* path,
    uint16_t len
);

extern bool eFile_Read(
    char const* path,
    uint16_t len,
    uint8_t buf[/*buf_len*/],
    uint32_t buf_len
);

extern bool eFile_ReadAll(
    char const* path,
    uint16_t len,
    void (*func)(char)
);

extern bool eFile_Append(
    char const* path,
    uint16_t len,
    uint8_t buf[/*buf_len*/],
    uint32_t buf_len
);

extern bool eFile_Delete(
    char const* path,
    uint16_t len
);

extern bool eFile_DirList(
    char const* path,
    uint16_t len,
    void (*func)(const char name[8], const char ext[3])
);

extern bool eFile_Flush();
