/* ntfs-3g treats a failed setgroups() as fatal while dropping privileges, but a
 * private user namespace denies that call. It has no privileges to drop there,
 * so this no-op is loaded into it for the fixture run only. */
#include <stddef.h>
#include <sys/types.h>

int setgroups(size_t size, const gid_t *list) {
    (void)size;
    (void)list;
    return 0;
}
