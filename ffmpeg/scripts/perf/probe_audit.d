/*
 * progenyof()-scoped dtrace audit for the vt-ferry perf gates.
 *
 * Pass the gate script's PID as $1 (the wrapper does this).
 * Every probe predicate uses /progenyof($1)/ so we see every
 * descendant of the gate — broker, smolvm, vt-ferry-worker,
 * helpers — regardless of who forks who or when. This avoids
 * the pid-provider trap (`-p PID` only follows one pid; if
 * the worker respawns mid-gate the trace silently goes blind).
 *
 * Probes:
 *   - syscall:::entry             — count syscalls by (execname, name)
 *   - syscall::write*:entry, ...  — sum byte volume on host-side write paths
 *   - syscall::read*:entry, ...   — sum byte volume on host-side read paths
 *   - profile-997                 — sample user stacks for hot-path discovery
 *
 * tick-150s prints aggregations and exits. Don't bump it past
 * the slowest gate's wallclock or you'll get partial output.
 *
 * For readable smolvm stacks, rebuild smolvm with debug symbols
 * (vendored release profile sets strip = true). Override at the
 * cargo CLI:
 *
 *     cd third_party/smolvm && \
 *       CARGO_PROFILE_RELEASE_DEBUG=true \
 *       CARGO_PROFILE_RELEASE_STRIP=none \
 *       cargo build --release --bin smolvm
 *
 * Without that, smolvm's own frames render as raw hex addresses
 * (libkrun + Hypervisor frames are still symbolicated since they
 * come from system-installed dylibs).
 */

#pragma D option quiet
#pragma D option destructive
#pragma D option dynvarsize=64m
#pragma D option strsize=128

syscall:::entry
/progenyof($1)/
{
    @sys_count[execname, probefunc] = count();
}

syscall::write*:entry,
syscall::pwrite*:entry,
syscall::writev*:entry,
syscall::sendto:entry,
syscall::sendmsg:entry
/progenyof($1)/
{
    @sys_write_bytes[execname, probefunc] = sum(arg2);
}

syscall::read*:entry,
syscall::pread*:entry,
syscall::readv*:entry,
syscall::recvfrom:entry,
syscall::recvmsg:entry
/progenyof($1)/
{
    @sys_read_bytes[execname, probefunc] = sum(arg2);
}

profile-997
/progenyof($1)/
{
    @stacks[execname, ustack(8)] = count();
    @cpu_samples[execname] = count();
}

tick-150s
{
    printf("\n=== CPU sample counts by execname (997Hz) ===\n");
    printa("%-24s %@10d\n", @cpu_samples);

    printf("\n=== syscall counts by (execname, syscall) — top 40 ===\n");
    trunc(@sys_count, 40);
    printa("%-24s %-30s %@10d\n", @sys_count);

    printf("\n=== write-side byte volume by (execname, syscall) ===\n");
    printa("%-24s %-30s %@14d\n", @sys_write_bytes);

    printf("\n=== read-side byte volume by (execname, syscall) ===\n");
    printa("%-24s %-30s %@14d\n", @sys_read_bytes);

    printf("\n=== top user stacks by execname (sampled @ 997Hz) ===\n");
    trunc(@stacks, 25);
    printa(@stacks);
    exit(0);
}
