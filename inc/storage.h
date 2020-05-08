#pragma once

#include <stdint.h>

typedef struct {
    uint8_t drive_num;
    uint64_t size_in_sectors;
} EDiskStorage;

extern EDiskStorage new_edisk_storage(uint8_t drive_num, uint64_t size_in_sectors);

// To test:
extern uint64_t sector_sum(EDiskStorage* _Nonnull storage, uint64_t size_in_sectors);
