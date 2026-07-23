/// <reference path="../pb_data/types.d.ts" />

// Nightly retention: raw activity snapshots and network connections older
// than RETENTION_DAYS are deleted. Session records, breaks, and the stamped
// net_loss_seconds/total_break_seconds aggregates are kept forever, so
// attendance and time reports for old dates keep working — only the
// per-30s/per-60s raw telemetry ages out.
//
// Volume context (July 2026): ~5k snapshots + ~20k network rows per day.
// Runs at 21:00 UTC (02:45 NPT, outside office hours). Deletion is batched
// and capped per run; a backlog just drains over successive nights.
cronAdd("retention_cleanup", "0 21 * * *", () => {
    const RETENTION_DAYS = 90;
    const BATCH = 500;
    const MAX_DELETES_PER_RUN = 50000; // per collection

    const cutoff = new Date(Date.now() - RETENTION_DAYS * 24 * 60 * 60 * 1000)
        .toISOString()
        .replace("T", " ");

    for (const collection of ["activity_snapshots", "network_connections"]) {
        let deleted = 0;
        try {
            while (deleted < MAX_DELETES_PER_RUN) {
                const batch = $app.findRecordsByFilter(
                    collection,
                    `timestamp < '${cutoff}'`,
                    "",
                    BATCH,
                    0
                );
                if (batch.length === 0) {
                    break;
                }
                for (const record of batch) {
                    try {
                        $app.delete(record);
                        deleted++;
                    } catch (e) {
                        console.error(`[retention] failed to delete ${collection}/${record.id}:`, e);
                    }
                }
            }
        } catch (e) {
            console.error(`[retention] sweep failed for ${collection}:`, e);
        }
        if (deleted > 0) {
            console.log(`[retention] deleted ${deleted} ${collection} rows older than ${cutoff}`);
        }
    }
});
