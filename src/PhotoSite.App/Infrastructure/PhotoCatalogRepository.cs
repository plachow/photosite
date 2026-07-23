using System.Text.Json;
using Microsoft.Data.Sqlite;
using PhotoSite.Domain;

namespace PhotoSite.Infrastructure;

public sealed class PhotoCatalogRepository
{
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

        await using var indexCommand = connection.CreateCommand();
        indexCommand.CommandText =
            """
            CREATE INDEX IF NOT EXISTS ix_photos_root_taken
                ON photos(root_path, taken_at_ticks);
            """;
        await indexCommand.ExecuteNonQueryAsync(cancellationToken);
    }

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
        command.CommandText =
            """
            INSERT INTO photos (
                path, root_path, file_name, extension, length,
                modified_utc_ticks, rating, scan_id,
                taken_at_ticks, taken_at_source, metadata_indexed)
            VALUES (
                $path, $root, $name, $extension, $length,
                $modified, $rating, $scan,
                $takenAt, $takenAtSource, $metadataIndexed)
            ON CONFLICT(path) DO UPDATE SET
                root_path = excluded.root_path,
                file_name = excluded.file_name,
                extension = excluded.extension,
                length = excluded.length,
                modified_utc_ticks = excluded.modified_utc_ticks,
                scan_id = excluded.scan_id,
                taken_at_ticks = excluded.taken_at_ticks,
                taken_at_source = excluded.taken_at_source,
                metadata_indexed = excluded.metadata_indexed;
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
            metadataIndexed.Value = record.MetadataIndexed ? 1 : 0;
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
        command.CommandText = "DELETE FROM photos WHERE path = $path;";
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
            """
            SELECT path, root_path, file_name, extension, length,
                   modified_utc_ticks, rating, scan_id,
                   taken_at_ticks, taken_at_source, metadata_indexed
            FROM photos
            WHERE root_path = $root
            ORDER BY file_name COLLATE NOCASE, path COLLATE NOCASE;
            """;
        command.Parameters.AddWithValue("$root", rootPath);

        await using var reader = await command.ExecuteReaderAsync(cancellationToken);
        while (await reader.ReadAsync(cancellationToken))
        {
            result.Add(new PhotoRecord(
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
                reader.GetInt32(10) != 0));
        }

        return result;
    }

    public async Task UpdateRatingAsync(
        string path,
        int rating,
        CancellationToken cancellationToken = default)
    {
        ArgumentOutOfRangeException.ThrowIfLessThan(rating, 0);
        ArgumentOutOfRangeException.ThrowIfGreaterThan(rating, 5);

        await using var connection = await OpenConnectionAsync(cancellationToken);
        await using var transaction = await connection.BeginTransactionAsync(cancellationToken);

        await using (var update = connection.CreateCommand())
        {
            update.Transaction = (SqliteTransaction)transaction;
            update.CommandText = "UPDATE photos SET rating = $rating WHERE path = $path;";
            update.Parameters.AddWithValue("$rating", rating);
            update.Parameters.AddWithValue("$path", path);
            await update.ExecuteNonQueryAsync(cancellationToken);
        }

        await using (var outbox = connection.CreateCommand())
        {
            outbox.Transaction = (SqliteTransaction)transaction;
            outbox.CommandText =
                """
                INSERT INTO metadata_outbox(path, kind, payload_json, created_utc)
                VALUES($path, 'rating', $payload, $created);
                """;
            outbox.Parameters.AddWithValue("$path", path);
            outbox.Parameters.AddWithValue(
                "$payload",
                JsonSerializer.Serialize(new { rating }));
            outbox.Parameters.AddWithValue("$created", DateTime.UtcNow.ToString("O"));
            await outbox.ExecuteNonQueryAsync(cancellationToken);
        }

        await transaction.CommitAsync(cancellationToken);
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

    private static async Task EnsureColumnAsync(
        SqliteConnection connection,
        string columnName,
        string declaration,
        CancellationToken cancellationToken)
    {
        await using (var inspect = connection.CreateCommand())
        {
            inspect.CommandText = "PRAGMA table_info(photos);";
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
            $"ALTER TABLE photos ADD COLUMN {columnName} {declaration};";
        await alter.ExecuteNonQueryAsync(cancellationToken);
    }
}
