using System.Text.Json;
using Microsoft.Data.Sqlite;
using PhotoSite.Domain;

namespace PhotoSite.Infrastructure;

public sealed partial class PhotoCatalogRepository
{
    private const string PhotoColumns =
        """
        path, root_path, file_name, extension, length,
        modified_utc_ticks, rating, scan_id,
        taken_at_ticks, taken_at_source, metadata_indexed,
        title, description, latitude, longitude,
        color_label, flag, keywords, pixel_width, pixel_height,
        camera, lens, focal_length, aperture, exposure_seconds, iso,
        description_en
        """;

    private readonly string connectionString;

    public PhotoCatalogRepository(string databasePath)
    {
        connectionString = new SqliteConnectionStringBuilder
        {
            DataSource = databasePath,
            Mode = SqliteOpenMode.ReadWriteCreate,
            Cache = SqliteCacheMode.Shared
        }.ToString();
    }

    public async Task InitializeAsync(CancellationToken cancellationToken = default)
    {
        await using var connection = await OpenConnectionAsync(cancellationToken);
        await using var command = connection.CreateCommand();
        command.CommandText =
            """
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA busy_timeout = 5000;

            CREATE TABLE IF NOT EXISTS photos (
                path                TEXT PRIMARY KEY COLLATE NOCASE,
                root_path           TEXT NOT NULL COLLATE NOCASE,
                file_name           TEXT NOT NULL COLLATE NOCASE,
                extension           TEXT NOT NULL COLLATE NOCASE,
                length              INTEGER NOT NULL,
                modified_utc_ticks  INTEGER NOT NULL,
                rating              INTEGER NOT NULL DEFAULT 0
                                    CHECK (rating BETWEEN 0 AND 5),
                scan_id             INTEGER NOT NULL,
                taken_at_ticks      INTEGER NULL,
                taken_at_source     INTEGER NOT NULL DEFAULT 0,
                metadata_indexed    INTEGER NOT NULL DEFAULT 0
            );

            CREATE INDEX IF NOT EXISTS ix_photos_root_name
                ON photos(root_path, file_name);

            CREATE TABLE IF NOT EXISTS edit_recipes (
                path         TEXT PRIMARY KEY COLLATE NOCASE,
                recipe_json  TEXT NOT NULL,
                updated_utc  TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS metadata_outbox (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                path         TEXT NOT NULL COLLATE NOCASE,
                kind         TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                created_utc  TEXT NOT NULL,
                attempts     INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS app_settings (
                key          TEXT PRIMARY KEY COLLATE NOCASE,
                value        TEXT NOT NULL,
                updated_utc  TEXT NOT NULL
            );
            """;
        await command.ExecuteNonQueryAsync(cancellationToken);

        await EnsureColumnAsync(
            connection,
            "taken_at_ticks",
            "INTEGER NULL",
            cancellationToken);
        await EnsureColumnAsync(
            connection,
            "taken_at_source",
            "INTEGER NOT NULL DEFAULT 0",
            cancellationToken);
        await EnsureColumnAsync(
            connection,
            "metadata_indexed",
            "INTEGER NOT NULL DEFAULT 0",
            cancellationToken);
        await EnsureColumnAsync(
            connection,
            "title",
            "TEXT NULL",
            cancellationToken);
        await EnsureColumnAsync(
            connection,
            "description",
            "TEXT NULL",
            cancellationToken);
        await EnsureColumnAsync(
            connection,
            "latitude",
            "REAL NULL",
            cancellationToken);
        await EnsureColumnAsync(
            connection,
            "longitude",
            "REAL NULL",
            cancellationToken);

        // Organisation and shooting data, added after the first release; the
        // photos table is migrated in place so existing catalogues keep their
        // ratings, recipes and thumbnails.
        foreach (var (column, declaration) in NewerColumns)
        {
            await EnsureColumnAsync(
                connection,
                column,
                declaration,
                cancellationToken);
        }

        await using var indexCommand = connection.CreateCommand();
        indexCommand.CommandText =
            """
            CREATE INDEX IF NOT EXISTS ix_photos_root_taken
                ON photos(root_path, taken_at_ticks);

            CREATE TABLE IF NOT EXISTS presets (
                kind         TEXT NOT NULL COLLATE NOCASE,
                name         TEXT NOT NULL COLLATE NOCASE,
                payload_json TEXT NOT NULL,
                updated_utc  TEXT NOT NULL,
                PRIMARY KEY (kind, name)
            );

            CREATE TABLE IF NOT EXISTS people (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                name         TEXT NOT NULL COLLATE NOCASE,
                created_utc  TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS faces (
                id                   INTEGER PRIMARY KEY AUTOINCREMENT,
                path                 TEXT NOT NULL COLLATE NOCASE,
                x                    REAL NOT NULL,
                y                    REAL NOT NULL,
                w                    REAL NOT NULL,
                h                    REAL NOT NULL,
                confidence           REAL NOT NULL,
                embedding            BLOB NOT NULL,
                person_id            INTEGER NULL,
                suggested_person_id  INTEGER NULL,
                created_utc          TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS ix_faces_path ON faces(path);
            CREATE INDEX IF NOT EXISTS ix_faces_person ON faces(person_id);

            CREATE TABLE IF NOT EXISTS face_scans (
                path                TEXT PRIMARY KEY COLLATE NOCASE,
                modified_utc_ticks  INTEGER NOT NULL,
                face_count          INTEGER NOT NULL,
                scanned_utc         TEXT NOT NULL
            );
            """;
        await indexCommand.ExecuteNonQueryAsync(cancellationToken);

        // Face suggestions arrived after the first face-recognition release;
        // an existing faces table is migrated in place.
        await EnsureColumnAsync(
            connection,
            "faces",
            "suggested_person_id",
            "INTEGER NULL",
            cancellationToken);
    }

    private static readonly (string Column, string Declaration)[] NewerColumns =
    [
        ("color_label", "INTEGER NOT NULL DEFAULT 0"),
        ("flag", "INTEGER NOT NULL DEFAULT 0"),
        ("keywords", "TEXT NULL"),
        ("pixel_width", "INTEGER NULL"),
        ("pixel_height", "INTEGER NULL"),
        ("camera", "TEXT NULL"),
        ("lens", "TEXT NULL"),
        ("focal_length", "REAL NULL"),
        ("aperture", "REAL NULL"),
        ("exposure_seconds", "REAL NULL"),
        ("iso", "INTEGER NULL"),
        ("description_en", "TEXT NULL")
    ];

    public async Task UpsertBatchAsync(
        IReadOnlyCollection<PhotoRecord> records,
        CancellationToken cancellationToken)
    {
        if (records.Count == 0)
        {
            return;
        }

        await using var connection = await OpenConnectionAsync(cancellationToken);
        await using var transaction = await connection.BeginTransactionAsync(cancellationToken);
        await using var command = connection.CreateCommand();
        command.Transaction = (SqliteTransaction)transaction;
        // A re-scan must not clobber organisation the user did inside
        // PhotoSite, so descriptive columns are only taken from the file when
        // the file itself changed or a newer reader learned to see more.
        const string acceptFromFile =
            """
            photos.modified_utc_ticks <> excluded.modified_utc_ticks
            OR photos.length <> excluded.length
            OR photos.metadata_indexed < excluded.metadata_indexed
            """;
        command.CommandText =
            $"""
            INSERT INTO photos (
                {PhotoColumns})
            VALUES (
                $path, $root, $name, $extension, $length,
                $modified, $rating, $scan,
                $takenAt, $takenAtSource, $metadataIndexed,
                $title, $description, $latitude, $longitude,
                $colorLabel, $flag, $keywords, $pixelWidth, $pixelHeight,
                $camera, $lens, $focalLength, $aperture, $exposureSeconds, $iso,
                $descriptionEn)
            ON CONFLICT(path) DO UPDATE SET
                root_path = excluded.root_path,
                file_name = excluded.file_name,
                extension = excluded.extension,
                length = excluded.length,
                modified_utc_ticks = excluded.modified_utc_ticks,
                scan_id = excluded.scan_id,
                taken_at_ticks = excluded.taken_at_ticks,
                taken_at_source = excluded.taken_at_source,
                metadata_indexed = excluded.metadata_indexed,
                pixel_width = excluded.pixel_width,
                pixel_height = excluded.pixel_height,
                camera = excluded.camera,
                lens = excluded.lens,
                focal_length = excluded.focal_length,
                aperture = excluded.aperture,
                exposure_seconds = excluded.exposure_seconds,
                iso = excluded.iso,
                rating = CASE
                    WHEN {acceptFromFile}
                    THEN excluded.rating ELSE photos.rating END,
                title = CASE
                    WHEN {acceptFromFile}
                    THEN excluded.title ELSE photos.title END,
                description = CASE
                    WHEN {acceptFromFile}
                    THEN excluded.description ELSE photos.description END,
                latitude = CASE
                    WHEN {acceptFromFile}
                    THEN excluded.latitude ELSE photos.latitude END,
                longitude = CASE
                    WHEN {acceptFromFile}
                    THEN excluded.longitude ELSE photos.longitude END,
                color_label = CASE
                    WHEN {acceptFromFile}
                    THEN excluded.color_label ELSE photos.color_label END,
                keywords = CASE
                    WHEN {acceptFromFile}
                    THEN excluded.keywords ELSE photos.keywords END;
            """;

        var path = command.Parameters.Add("$path", SqliteType.Text);
        var root = command.Parameters.Add("$root", SqliteType.Text);
        var name = command.Parameters.Add("$name", SqliteType.Text);
        var extension = command.Parameters.Add("$extension", SqliteType.Text);
        var length = command.Parameters.Add("$length", SqliteType.Integer);
        var modified = command.Parameters.Add("$modified", SqliteType.Integer);
        var rating = command.Parameters.Add("$rating", SqliteType.Integer);
        var scan = command.Parameters.Add("$scan", SqliteType.Integer);
        var takenAt = command.Parameters.Add("$takenAt", SqliteType.Integer);
        var takenAtSource = command.Parameters.Add(
            "$takenAtSource",
            SqliteType.Integer);
        var metadataIndexed = command.Parameters.Add(
            "$metadataIndexed",
            SqliteType.Integer);
        var title = command.Parameters.Add("$title", SqliteType.Text);
        var description = command.Parameters.Add(
            "$description",
            SqliteType.Text);
        var latitude = command.Parameters.Add("$latitude", SqliteType.Real);
        var longitude = command.Parameters.Add("$longitude", SqliteType.Real);
        var colorLabel = command.Parameters.Add("$colorLabel", SqliteType.Integer);
        var flag = command.Parameters.Add("$flag", SqliteType.Integer);
        var keywords = command.Parameters.Add("$keywords", SqliteType.Text);
        var pixelWidth = command.Parameters.Add("$pixelWidth", SqliteType.Integer);
        var pixelHeight = command.Parameters.Add("$pixelHeight", SqliteType.Integer);
        var camera = command.Parameters.Add("$camera", SqliteType.Text);
        var lens = command.Parameters.Add("$lens", SqliteType.Text);
        var focalLength = command.Parameters.Add("$focalLength", SqliteType.Real);
        var aperture = command.Parameters.Add("$aperture", SqliteType.Real);
        var exposureSeconds = command.Parameters.Add(
            "$exposureSeconds",
            SqliteType.Real);
        var iso = command.Parameters.Add("$iso", SqliteType.Integer);
        // The English description lives only in the catalogue - a scan never
        // reads one from the file, so the upsert must never replace it either;
        // like the flag, it is deliberately absent from the UPDATE clause.
        var descriptionEn = command.Parameters.Add(
            "$descriptionEn",
            SqliteType.Text);

        foreach (var record in records)
        {
            cancellationToken.ThrowIfCancellationRequested();
            path.Value = record.Path;
            root.Value = record.RootPath;
            name.Value = record.FileName;
            extension.Value = record.Extension;
            length.Value = record.Length;
            modified.Value = record.ModifiedUtcTicks;
            rating.Value = record.Rating;
            scan.Value = record.ScanId;
            takenAt.Value = record.TakenAtTicks is { } ticks
                ? ticks
                : DBNull.Value;
            takenAtSource.Value = (int)record.TakenAtSource;
            metadataIndexed.Value = record.MetadataVersion;
            title.Value = (object?)record.Title ?? DBNull.Value;
            description.Value = (object?)record.Description ?? DBNull.Value;
            latitude.Value = record.Latitude is { } lat
                ? lat
                : DBNull.Value;
            longitude.Value = record.Longitude is { } lon
                ? lon
                : DBNull.Value;
            colorLabel.Value = (int)record.ColorLabel;
            flag.Value = (int)record.Flag;
            keywords.Value = (object?)record.Keywords ?? DBNull.Value;
            pixelWidth.Value = record.PixelWidth is { } pw ? pw : DBNull.Value;
            pixelHeight.Value = record.PixelHeight is { } ph ? ph : DBNull.Value;
            camera.Value = (object?)record.Camera ?? DBNull.Value;
            lens.Value = (object?)record.Lens ?? DBNull.Value;
            focalLength.Value = record.FocalLength is { } fl ? fl : DBNull.Value;
            aperture.Value = record.Aperture is { } av ? av : DBNull.Value;
            exposureSeconds.Value = record.ExposureSeconds is { } es
                ? es
                : DBNull.Value;
            iso.Value = record.Iso is { } isoValue ? isoValue : DBNull.Value;
            descriptionEn.Value = (object?)record.DescriptionEn ?? DBNull.Value;
            await command.ExecuteNonQueryAsync(cancellationToken);
        }

        await transaction.CommitAsync(cancellationToken);
    }

    public async Task CompleteScanAsync(
        string rootPath,
        long scanId,
        bool includeSubfolders,
        CancellationToken cancellationToken)
    {
        await using var connection = await OpenConnectionAsync(cancellationToken);
        await using var command = connection.CreateCommand();
        command.CommandText = includeSubfolders
            ? """
              DELETE FROM photos
              WHERE root_path = $root AND scan_id <> $scan;
              """
            : """
              DELETE FROM photos
              WHERE root_path = $root
                AND scan_id <> $scan
                AND instr(
                    substr(path, length($prefix) + 1),
                    $separator) = 0;
              """;
        command.Parameters.AddWithValue("$root", rootPath);
        command.Parameters.AddWithValue("$scan", scanId);
        if (!includeSubfolders)
        {
            command.Parameters.AddWithValue(
                "$prefix",
                rootPath.EndsWith(Path.DirectorySeparatorChar)
                    ? rootPath
                    : rootPath + Path.DirectorySeparatorChar);
            command.Parameters.AddWithValue(
                "$separator",
                Path.DirectorySeparatorChar.ToString());
        }

        await command.ExecuteNonQueryAsync(cancellationToken);
    }

    public async Task DeleteByPathsAsync(
        IReadOnlyCollection<string> paths,
        CancellationToken cancellationToken)
    {
        if (paths.Count == 0)
        {
            return;
        }

        await using var connection = await OpenConnectionAsync(cancellationToken);
        await using var transaction = await connection.BeginTransactionAsync(
            cancellationToken);
        await using var command = connection.CreateCommand();
        command.Transaction = (SqliteTransaction)transaction;
        command.CommandText =
            """
            DELETE FROM photos WHERE path = $path;
            DELETE FROM faces WHERE path = $path;
            DELETE FROM face_scans WHERE path = $path;
            """;
        var path = command.Parameters.Add("$path", SqliteType.Text);

        foreach (var value in paths)
        {
            cancellationToken.ThrowIfCancellationRequested();
            path.Value = value;
            await command.ExecuteNonQueryAsync(cancellationToken);
        }

        await transaction.CommitAsync(cancellationToken);
    }

    public async Task<IReadOnlyList<PhotoRecord>> GetByRootAsync(
        string rootPath,
        CancellationToken cancellationToken)
    {
        var result = new List<PhotoRecord>();
        await using var connection = await OpenConnectionAsync(cancellationToken);
        await using var command = connection.CreateCommand();
        command.CommandText =
            $"""
            SELECT {PhotoColumns}
            FROM photos
            WHERE root_path = $root
            ORDER BY file_name COLLATE NOCASE, path COLLATE NOCASE;
            """;
        command.Parameters.AddWithValue("$root", rootPath);

        await using var reader = await command.ExecuteReaderAsync(cancellationToken);
        while (await reader.ReadAsync(cancellationToken))
        {
            result.Add(ReadPhotoRecord(reader));
        }

        return result;
    }

    public async Task<PhotoRecord?> GetByPathAsync(
        string path,
        CancellationToken cancellationToken = default)
    {
        await using var connection = await OpenConnectionAsync(cancellationToken);
        await using var command = connection.CreateCommand();
        command.CommandText =
            $"""
            SELECT {PhotoColumns}
            FROM photos
            WHERE path = $path;
            """;
        command.Parameters.AddWithValue("$path", path);

        await using var reader = await command.ExecuteReaderAsync(cancellationToken);
        if (!await reader.ReadAsync(cancellationToken))
        {
            return null;
        }

        return ReadPhotoRecord(reader);
    }

    private static PhotoRecord ReadPhotoRecord(SqliteDataReader reader) =>
        new(
            reader.GetString(0),
            reader.GetString(1),
            reader.GetString(2),
            reader.GetString(3),
            reader.GetInt64(4),
            reader.GetInt64(5),
            reader.GetInt32(6),
            reader.GetInt64(7),
            reader.IsDBNull(8) ? null : reader.GetInt64(8),
            (PhotoDateSource)reader.GetInt32(9),
            reader.GetInt32(10),
            reader.IsDBNull(11) ? null : reader.GetString(11),
            reader.IsDBNull(12) ? null : reader.GetString(12),
            reader.IsDBNull(13) ? null : reader.GetDouble(13),
            reader.IsDBNull(14) ? null : reader.GetDouble(14),
            (ColorLabel)reader.GetInt32(15),
            (PhotoFlag)reader.GetInt32(16),
            reader.IsDBNull(17) ? null : reader.GetString(17),
            reader.IsDBNull(18) ? null : reader.GetInt32(18),
            reader.IsDBNull(19) ? null : reader.GetInt32(19),
            reader.IsDBNull(20) ? null : reader.GetString(20),
            reader.IsDBNull(21) ? null : reader.GetString(21),
            reader.IsDBNull(22) ? null : reader.GetDouble(22),
            reader.IsDBNull(23) ? null : reader.GetDouble(23),
            reader.IsDBNull(24) ? null : reader.GetDouble(24),
            reader.IsDBNull(25) ? null : reader.GetInt32(25),
            reader.IsDBNull(26) ? null : reader.GetString(26));

    public event Action? MetadataOutboxChanged;

    public async Task UpdateRatingAsync(
        string path,
        int rating,
        CancellationToken cancellationToken = default)
    {
        ArgumentOutOfRangeException.ThrowIfLessThan(rating, 0);
        ArgumentOutOfRangeException.ThrowIfGreaterThan(rating, 5);
        await UpdateMetadataFieldAsync(
            path,
            "UPDATE photos SET rating = $value WHERE path = $path;",
            rating,
            "rating",
            JsonSerializer.Serialize(new { rating }),
            cancellationToken);
    }

    public Task UpdateTitleAsync(
        string path,
        string? title,
        CancellationToken cancellationToken = default) =>
        UpdateMetadataFieldAsync(
            path,
            "UPDATE photos SET title = $value WHERE path = $path;",
            (object?)title ?? DBNull.Value,
            "title",
            JsonSerializer.Serialize(new { title }),
            cancellationToken);

    public Task UpdateColorLabelAsync(
        string path,
        ColorLabel label,
        CancellationToken cancellationToken = default) =>
        UpdateMetadataFieldAsync(
            path,
            "UPDATE photos SET color_label = $value WHERE path = $path;",
            (int)label,
            "label",
            JsonSerializer.Serialize(new { label = label.ToXmpName() }),
            cancellationToken);

    public Task UpdateKeywordsAsync(
        string path,
        string? keywords,
        CancellationToken cancellationToken = default) =>
        UpdateMetadataFieldAsync(
            path,
            "UPDATE photos SET keywords = $value WHERE path = $path;",
            (object?)keywords ?? DBNull.Value,
            "keywords",
            JsonSerializer.Serialize(new { keywords }),
            cancellationToken);

    /// <summary>
    /// Pick and reject live only in the catalogue: they are a working state
    /// for a culling session, not something to write back into the file.
    /// </summary>
    public async Task UpdateFlagAsync(
        string path,
        PhotoFlag flag,
        CancellationToken cancellationToken = default)
    {
        await using var connection = await OpenConnectionAsync(cancellationToken);
        await using var command = connection.CreateCommand();
        command.CommandText =
            "UPDATE photos SET flag = $value WHERE path = $path;";
        command.Parameters.AddWithValue("$value", (int)flag);
        command.Parameters.AddWithValue("$path", path);
        await command.ExecuteNonQueryAsync(cancellationToken);
    }

    public Task UpdateDescriptionAsync(
        string path,
        string? description,
        CancellationToken cancellationToken = default) =>
        UpdateMetadataFieldAsync(
            path,
            "UPDATE photos SET description = $value WHERE path = $path;",
            (object?)description ?? DBNull.Value,
            "description",
            JsonSerializer.Serialize(new { description }),
            cancellationToken);

    /// <summary>
    /// The English description lives only in the catalogue, as a second
    /// searchable language; the file keeps the primary-language description,
    /// so no outbox entry is queued.
    /// </summary>
    public async Task UpdateDescriptionEnAsync(
        string path,
        string? description,
        CancellationToken cancellationToken = default)
    {
        await using var connection = await OpenConnectionAsync(cancellationToken);
        await using var command = connection.CreateCommand();
        command.CommandText =
            "UPDATE photos SET description_en = $value WHERE path = $path;";
        command.Parameters.AddWithValue(
            "$value",
            (object?)description ?? DBNull.Value);
        command.Parameters.AddWithValue("$path", path);
        await command.ExecuteNonQueryAsync(cancellationToken);
    }

    public async Task UpdateLocationAsync(
        string path,
        double? latitude,
        double? longitude,
        CancellationToken cancellationToken = default)
    {
        await using var connection = await OpenConnectionAsync(cancellationToken);
        await using var transaction = await connection.BeginTransactionAsync(
            cancellationToken);

        await using (var update = connection.CreateCommand())
        {
            update.Transaction = (SqliteTransaction)transaction;
            update.CommandText =
                """
                UPDATE photos
                SET latitude = $latitude, longitude = $longitude
                WHERE path = $path;
                """;
            update.Parameters.AddWithValue(
                "$latitude",
                latitude is { } lat ? lat : DBNull.Value);
            update.Parameters.AddWithValue(
                "$longitude",
                longitude is { } lon ? lon : DBNull.Value);
            update.Parameters.AddWithValue("$path", path);
            await update.ExecuteNonQueryAsync(cancellationToken);
        }

        await EnqueueOutboxAsync(
            connection,
            (SqliteTransaction)transaction,
            path,
            "location",
            JsonSerializer.Serialize(new { latitude, longitude }),
            cancellationToken);
        await transaction.CommitAsync(cancellationToken);
        MetadataOutboxChanged?.Invoke();
    }

    private async Task UpdateMetadataFieldAsync(
        string path,
        string updateSql,
        object value,
        string kind,
        string payloadJson,
        CancellationToken cancellationToken)
    {
        await using var connection = await OpenConnectionAsync(cancellationToken);
        await using var transaction = await connection.BeginTransactionAsync(
            cancellationToken);

        await using (var update = connection.CreateCommand())
        {
            update.Transaction = (SqliteTransaction)transaction;
            update.CommandText = updateSql;
            update.Parameters.AddWithValue("$value", value);
            update.Parameters.AddWithValue("$path", path);
            await update.ExecuteNonQueryAsync(cancellationToken);
        }

        await EnqueueOutboxAsync(
            connection,
            (SqliteTransaction)transaction,
            path,
            kind,
            payloadJson,
            cancellationToken);
        await transaction.CommitAsync(cancellationToken);
        MetadataOutboxChanged?.Invoke();
    }

    private static async Task EnqueueOutboxAsync(
        SqliteConnection connection,
        SqliteTransaction transaction,
        string path,
        string kind,
        string payloadJson,
        CancellationToken cancellationToken)
    {
        await using var outbox = connection.CreateCommand();
        outbox.Transaction = transaction;
        outbox.CommandText =
            """
            INSERT INTO metadata_outbox(path, kind, payload_json, created_utc)
            VALUES($path, $kind, $payload, $created);
            """;
        outbox.Parameters.AddWithValue("$path", path);
        outbox.Parameters.AddWithValue("$kind", kind);
        outbox.Parameters.AddWithValue("$payload", payloadJson);
        outbox.Parameters.AddWithValue("$created", DateTime.UtcNow.ToString("O"));
        await outbox.ExecuteNonQueryAsync(cancellationToken);
    }

    public async Task<IReadOnlyList<MetadataOutboxEntry>> GetPendingMetadataAsync(
        int maxAttempts,
        CancellationToken cancellationToken = default)
    {
        var result = new List<MetadataOutboxEntry>();
        await using var connection = await OpenConnectionAsync(cancellationToken);
        await using var command = connection.CreateCommand();
        command.CommandText =
            """
            SELECT id, path, kind, payload_json, attempts
            FROM metadata_outbox
            WHERE attempts < $maxAttempts
            ORDER BY id;
            """;
        command.Parameters.AddWithValue("$maxAttempts", maxAttempts);

        await using var reader = await command.ExecuteReaderAsync(cancellationToken);
        while (await reader.ReadAsync(cancellationToken))
        {
            result.Add(new MetadataOutboxEntry(
                reader.GetInt64(0),
                reader.GetString(1),
                reader.GetString(2),
                reader.GetString(3),
                reader.GetInt32(4)));
        }

        return result;
    }

    public async Task DeleteMetadataOutboxEntriesAsync(
        IReadOnlyCollection<long> ids,
        CancellationToken cancellationToken = default)
    {
        if (ids.Count == 0)
        {
            return;
        }

        await using var connection = await OpenConnectionAsync(cancellationToken);
        await using var transaction = await connection.BeginTransactionAsync(
            cancellationToken);
        await using var command = connection.CreateCommand();
        command.Transaction = (SqliteTransaction)transaction;
        command.CommandText = "DELETE FROM metadata_outbox WHERE id = $id;";
        var id = command.Parameters.Add("$id", SqliteType.Integer);

        foreach (var value in ids)
        {
            cancellationToken.ThrowIfCancellationRequested();
            id.Value = value;
            await command.ExecuteNonQueryAsync(cancellationToken);
        }

        await transaction.CommitAsync(cancellationToken);
    }

    public async Task IncrementMetadataOutboxAttemptsAsync(
        IReadOnlyCollection<long> ids,
        CancellationToken cancellationToken = default)
    {
        if (ids.Count == 0)
        {
            return;
        }

        await using var connection = await OpenConnectionAsync(cancellationToken);
        await using var transaction = await connection.BeginTransactionAsync(
            cancellationToken);
        await using var command = connection.CreateCommand();
        command.Transaction = (SqliteTransaction)transaction;
        command.CommandText =
            """
            UPDATE metadata_outbox
            SET attempts = attempts + 1
            WHERE id = $id;
            """;
        var id = command.Parameters.Add("$id", SqliteType.Integer);

        foreach (var value in ids)
        {
            cancellationToken.ThrowIfCancellationRequested();
            id.Value = value;
            await command.ExecuteNonQueryAsync(cancellationToken);
        }

        await transaction.CommitAsync(cancellationToken);
    }

    public async Task UpdateFileStampAsync(
        string path,
        long length,
        long modifiedUtcTicks,
        CancellationToken cancellationToken = default)
    {
        await using var connection = await OpenConnectionAsync(cancellationToken);
        await using var command = connection.CreateCommand();
        command.CommandText =
            """
            UPDATE photos
            SET length = $length, modified_utc_ticks = $modified
            WHERE path = $path;
            """;
        command.Parameters.AddWithValue("$length", length);
        command.Parameters.AddWithValue("$modified", modifiedUtcTicks);
        command.Parameters.AddWithValue("$path", path);
        await command.ExecuteNonQueryAsync(cancellationToken);
    }

    public async Task<EditRecipe> GetEditRecipeAsync(
        string path,
        CancellationToken cancellationToken = default)
    {
        await using var connection = await OpenConnectionAsync(cancellationToken);
        await using var command = connection.CreateCommand();
        command.CommandText =
            """
            SELECT recipe_json
            FROM edit_recipes
            WHERE path = $path;
            """;
        command.Parameters.AddWithValue("$path", path);
        var json = await command.ExecuteScalarAsync(cancellationToken) as string;
        return json is null
            ? EditRecipe.Empty
            : JsonSerializer.Deserialize<EditRecipe>(json) ?? EditRecipe.Empty;
    }

    public async Task<IReadOnlyDictionary<string, EditRecipe>> GetEditRecipesByRootAsync(
        string rootPath,
        CancellationToken cancellationToken = default)
    {
        var result = new Dictionary<string, EditRecipe>(
            StringComparer.OrdinalIgnoreCase);
        await using var connection = await OpenConnectionAsync(cancellationToken);
        await using var command = connection.CreateCommand();
        command.CommandText =
            """
            SELECT edits.path, edits.recipe_json
            FROM edit_recipes AS edits
            INNER JOIN photos ON photos.path = edits.path
            WHERE photos.root_path = $root;
            """;
        command.Parameters.AddWithValue("$root", rootPath);

        await using var reader = await command.ExecuteReaderAsync(cancellationToken);
        while (await reader.ReadAsync(cancellationToken))
        {
            try
            {
                var recipe = JsonSerializer.Deserialize<EditRecipe>(
                    reader.GetString(1));
                if (recipe is not null)
                {
                    result[reader.GetString(0)] = recipe;
                }
            }
            catch (JsonException)
            {
                // A malformed recipe must not prevent the catalogue from opening.
            }
        }

        return result;
    }

    public async Task SaveEditRecipeAsync(
        string path,
        EditRecipe recipe,
        CancellationToken cancellationToken = default)
    {
        await using var connection = await OpenConnectionAsync(cancellationToken);
        await using var command = connection.CreateCommand();
        command.CommandText =
            """
            INSERT INTO edit_recipes(path, recipe_json, updated_utc)
            VALUES($path, $recipe, $updated)
            ON CONFLICT(path) DO UPDATE SET
                recipe_json = excluded.recipe_json,
                updated_utc = excluded.updated_utc;
            """;
        command.Parameters.AddWithValue("$path", path);
        command.Parameters.AddWithValue("$recipe", JsonSerializer.Serialize(recipe));
        command.Parameters.AddWithValue("$updated", DateTime.UtcNow.ToString("O"));
        await command.ExecuteNonQueryAsync(cancellationToken);
    }

    public async Task<string?> GetSettingAsync(
        string key,
        CancellationToken cancellationToken = default)
    {
        await using var connection = await OpenConnectionAsync(cancellationToken);
        await using var command = connection.CreateCommand();
        command.CommandText =
            """
            SELECT value
            FROM app_settings
            WHERE key = $key;
            """;
        command.Parameters.AddWithValue("$key", key);
        return await command.ExecuteScalarAsync(cancellationToken) as string;
    }

    public async Task SetSettingAsync(
        string key,
        string value,
        CancellationToken cancellationToken = default)
    {
        await using var connection = await OpenConnectionAsync(cancellationToken);
        await using var command = connection.CreateCommand();
        command.CommandText =
            """
            INSERT INTO app_settings(key, value, updated_utc)
            VALUES($key, $value, $updated)
            ON CONFLICT(key) DO UPDATE SET
                value = excluded.value,
                updated_utc = excluded.updated_utc;
            """;
        command.Parameters.AddWithValue("$key", key);
        command.Parameters.AddWithValue("$value", value);
        command.Parameters.AddWithValue("$updated", DateTime.UtcNow.ToString("O"));
        await command.ExecuteNonQueryAsync(cancellationToken);
    }

    public async Task<IReadOnlyList<(string Name, string Payload)>> GetPresetsAsync(
        string kind,
        CancellationToken cancellationToken = default)
    {
        var result = new List<(string, string)>();
        await using var connection = await OpenConnectionAsync(cancellationToken);
        await using var command = connection.CreateCommand();
        command.CommandText =
            """
            SELECT name, payload_json
            FROM presets
            WHERE kind = $kind
            ORDER BY name COLLATE NOCASE;
            """;
        command.Parameters.AddWithValue("$kind", kind);

        await using var reader = await command.ExecuteReaderAsync(cancellationToken);
        while (await reader.ReadAsync(cancellationToken))
        {
            result.Add((reader.GetString(0), reader.GetString(1)));
        }

        return result;
    }

    public async Task SavePresetAsync(
        string kind,
        string name,
        string payloadJson,
        CancellationToken cancellationToken = default)
    {
        await using var connection = await OpenConnectionAsync(cancellationToken);
        await using var command = connection.CreateCommand();
        command.CommandText =
            """
            INSERT INTO presets(kind, name, payload_json, updated_utc)
            VALUES($kind, $name, $payload, $updated)
            ON CONFLICT(kind, name) DO UPDATE SET
                payload_json = excluded.payload_json,
                updated_utc = excluded.updated_utc;
            """;
        command.Parameters.AddWithValue("$kind", kind);
        command.Parameters.AddWithValue("$name", name);
        command.Parameters.AddWithValue("$payload", payloadJson);
        command.Parameters.AddWithValue("$updated", DateTime.UtcNow.ToString("O"));
        await command.ExecuteNonQueryAsync(cancellationToken);
    }

    public async Task DeletePresetAsync(
        string kind,
        string name,
        CancellationToken cancellationToken = default)
    {
        await using var connection = await OpenConnectionAsync(cancellationToken);
        await using var command = connection.CreateCommand();
        command.CommandText =
            "DELETE FROM presets WHERE kind = $kind AND name = $name;";
        command.Parameters.AddWithValue("$kind", kind);
        command.Parameters.AddWithValue("$name", name);
        await command.ExecuteNonQueryAsync(cancellationToken);
    }

    private async Task<SqliteConnection> OpenConnectionAsync(
        CancellationToken cancellationToken)
    {
        var connection = new SqliteConnection(connectionString);
        await connection.OpenAsync(cancellationToken);

        await using var command = connection.CreateCommand();
        command.CommandText = "PRAGMA busy_timeout = 5000;";
        await command.ExecuteNonQueryAsync(cancellationToken);
        return connection;
    }

    private static Task EnsureColumnAsync(
        SqliteConnection connection,
        string columnName,
        string declaration,
        CancellationToken cancellationToken) =>
        EnsureColumnAsync(
            connection,
            "photos",
            columnName,
            declaration,
            cancellationToken);

    private static async Task EnsureColumnAsync(
        SqliteConnection connection,
        string tableName,
        string columnName,
        string declaration,
        CancellationToken cancellationToken)
    {
        await using (var inspect = connection.CreateCommand())
        {
            inspect.CommandText = $"PRAGMA table_info({tableName});";
            await using var reader = await inspect.ExecuteReaderAsync(cancellationToken);
            while (await reader.ReadAsync(cancellationToken))
            {
                if (string.Equals(
                        reader.GetString(1),
                        columnName,
                        StringComparison.OrdinalIgnoreCase))
                {
                    return;
                }
            }
        }

        await using var alter = connection.CreateCommand();
        alter.CommandText =
            $"ALTER TABLE {tableName} ADD COLUMN {columnName} {declaration};";
        await alter.ExecuteNonQueryAsync(cancellationToken);
    }
}
