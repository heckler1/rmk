MEMORY
{
  /* NOTE 1 K = 1 KiB = 1024 bytes */
  /* Matches code_partition in MoErgo's go60.dtsi. The MBR and SoftDevice stay in
     place below 0x26000, so the stock UF2 bootloader and ZMK keep working. */
  FLASH : ORIGIN = 0x00026000, LENGTH = 792K
  RAM : ORIGIN = 0x20000008, LENGTH = 255K
}
