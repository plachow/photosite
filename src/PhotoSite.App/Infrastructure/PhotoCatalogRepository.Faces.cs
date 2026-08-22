using Microsoft.Data.Sqlite;
using PhotoSite.Domain;

namespace PhotoSite.Infrastructure;

/// <summary>
/// The face side of the catalogue: detected faces with their embeddings,
/// the people they are assigned to, and the per-file scan marker that lets
/// an interrupted sweep skip everything it already covered.
/// </summary>
public sealed partial class PhotoCatalogRepository
{
    /// <summary>
    /// Which files have been face-scanned, and the file stamp the scan saw;
    /// a changed file is scanned again.
    /// </summary>
    public async Task<IReadOnlyDictionary<string, long>> GetFaceScanStatesAsync(
        CancellationToken cancellationToken = default)
    {
        await using var connection = await OpenConnectionAsync(cancellationToken);
        await using var command = connection.CreateCommand();
        command.CommandText =
            "SELECT path, modified_utc_ticks FROM face_scans;";
        var states = new Dictionary<string, long>(
            StringComparer.OrdinalIgnoreCase);
        await using var reader = await command.ExecuteReaderAsync(
            cancellationToken);
        while (await reader.ReadAsync(cancellationToken))
        {
            states[reader.GetString(0)] = reader.GetInt64(1);
        }

        return states;
    }

    /// <summary>
    /// Replaces every face of one photograph with a fresh detection result
    /// and stamps the scan, in one transaction.
    /// </summary>
    public async Task ReplaceFacesAsync(
        string path,
        long modifiedUtcTicks,
        IReadOnlyList<(double X, double Y, double Width, double Height,
            double Confidence, float[] Embedding, long? PersonId,
            long? SuggestedPersonId)> faces,
        CancellationToken cancellationToken = default)
    {
        await using var connection = await OpenConnectionAsync(cancellationToken);
        await using var transaction = await connection.BeginTransactionAsync(
            cancellationToken);

        await using (var delete = connection.CreateCommand())
        {
            delete.Transaction = (SqliteTransaction)transaction;
            delete.CommandText = "DELETE FROM faces WHERE path = $path;";
            delete.Parameters.AddWithValue("$path", path);
            await delete.ExecuteNonQueryAsync(cancellationToken);
        }

        if (faces.Count > 0)
        {
            await using var insert = connection.CreateCommand();
            insert.Transaction = (SqliteTransaction)transaction;
            insert.CommandText =
                """
                INSERT INTO faces (
                    path, x, y, w, h, confidence, embedding, person_id,
                    suggested_person_id, created_utc)
                VALUES (
                    $path, $x, $y, $w, $h, $confidence, $embedding, $person,
                    $suggested, $created);
                """;
            insert.Parameters.AddWithValue("$path", path);
            insert.Parameters.AddWithValue(
                "$created",
                DateTime.UtcNow.ToString("O"));
            var x = insert.Parameters.Add("$x", SqliteType.Real);
            var y = insert.Parameters.Add("$y", SqliteType.Real);
            var w = insert.Parameters.Add("$w", SqliteType.Real);
            var h = insert.Parameters.Add("$h", SqliteType.Real);
            var confidence = insert.Parameters.Add(
                "$confidence",
                SqliteType.Real);
            var embedding = insert.Parameters.Add(
                "$embedding",
                SqliteType.Blob);
            var person = insert.Parameters.Add("$person", SqliteType.Integer);
            var suggested = insert.Parameters.Add(
                "$suggested",
                SqliteType.Integer);

            foreach (var face in faces)
            {
                cancellationToken.ThrowIfCancellationRequested();
                x.Value = face.X;
                y.Value = face.Y;
                w.Value = face.Width;
                h.Value = face.Height;
                confidence.Value = face.Confidence;
                embedding.Value = EmbeddingToBlob(face.Embedding);
                person.Value = face.PersonId is { } personId
                    ? personId
                    : DBNull.Value;
                suggested.Value = face.SuggestedPersonId is { } suggestedId
                    ? suggestedId
                    : DBNull.Value;
                await insert.ExecuteNonQueryAsync(cancellationToken);
            }
        }

        await using (var stamp = connection.CreateCommand())
        {
            stamp.Transaction = (SqliteTransaction)transaction;
            stamp.CommandText =
                """
                INSERT INTO face_scans (
                    path, modified_utc_ticks, face_count, scanned_utc)
                VALUES ($path, $modified, $count, $scanned)
                ON CONFLICT(path) DO UPDATE SET
                    modified_utc_ticks = excluded.modified_utc_ticks,
                    face_count = excluded.face_count,
                    scanned_utc = excluded.scanned_utc;
                """;
            stamp.Parameters.AddWithValue("$path", path);
            stamp.Parameters.AddWithValue("$modified", modifiedUtcTicks);
            stamp.Parameters.AddWithValue("$count", faces.Count);
            stamp.Parameters.AddWithValue(
                "$scanned",
                DateTime.UtcNow.ToString("O"));
            await stamp.ExecuteNonQueryAsync(cancellationToken);
        }

        await transaction.CommitAsync(cancellationToken);
    }

    /// <summary>
    /// Faces with neither a person nor a pending suggestion - the pool the
    /// unnamed-group clustering works on.
    /// </summary>
    public Task<IReadOnlyList<FaceRecord>> GetUnassignedFacesAsync(
        CancellationToken cancellationToken = default) =>
        QueryFacesAsync(
            "WHERE person_id IS NULL AND suggested_person_id IS NULL",
            null,
            null,
            cancellationToken);

    public Task<IReadOnlyList<FaceRecord>> GetAssignedFacesAsync(
        CancellationToken cancellationToken = default) =>
        QueryFacesAsync(
            "WHERE person_id IS NOT NULL",
            null,
            null,
            cancellationToken);

    /// <summary>Borderline matches waiting for a yes or no.</summary>
    public Task<IReadOnlyList<FaceRecord>> GetSuggestedFacesAsync(
        CancellationToken cancellationToken = default) =>
        QueryFacesAsync(
            "WHERE person_id IS NULL AND suggested_person_id IS NOT NULL",
            null,
            null,
            cancellationToken);

    public Task<IReadOnlyList<FaceRecord>> GetFacesForPersonAsync(
        long personId,
        CancellationToken cancellationToken = default) =>
        QueryFacesAsync(
            "WHERE person_id = $person",
            personId,
            null,
            cancellationToken);

    public Task<IReadOnlyList<FaceRecord>> GetFacesForPathAsync(
        string path,
        CancellationToken cancellationToken = default) =>
        QueryFacesAsync(
            "WHERE path = $path",
            null,
            path,
            cancellationToken);

    /// <summary>
    /// Every photograph's named people in one query - the source for the
    /// gallery badges, the info panel's People row and the person filter.
    /// </summary>
    public async Task<IReadOnlyDictionary<string, IReadOnlyList<PersonTag>>>
        GetPeopleByPhotoAsync(CancellationToken cancellationToken = default)
    {
        await using var connection = await OpenConnectionAsync(cancellationToken);
        await using var command = connection.CreateCommand();
        command.CommandText =
            """
            SELECT DISTINCT faces.path, people.id, people.name
            FROM faces
            JOIN people ON people.id = faces.person_id
            ORDER BY people.name COLLATE NOCASE;
            """;
        var map = new Dictionary<string, IReadOnlyList<PersonTag>>(
            StringComparer.OrdinalIgnoreCase);
        await using var reader = await command.ExecuteReaderAsync(
            cancellationToken);
        while (await reader.ReadAsync(cancellationToken))
        {
            var path = reader.GetString(0);
            var tag = new PersonTag(reader.GetInt64(1), reader.GetString(2));
            if (map.TryGetValue(path, out var existing))
            {
                map[path] = [.. existing, tag];
            }
            else
            {
                map[path] = [tag];
            }
        }

        return map;
    }

    /// <summary>The photos a person appears in, for the gallery filter.</summary>
    public async Task<IReadOnlyList<string>> GetPersonPhotoPathsAsync(
        long personId,
        CancellationToken cancellationToken = default)
    {
        await using var connection = await OpenConnectionAsync(cancellationToken);
        await using var command = connection.CreateCommand();
        command.CommandText =
            "SELECT DISTINCT path FROM faces WHERE person_id = $person;";
        command.Parameters.AddWithValue("$person", personId);
        var paths = new List<string>();
        await using var reader = await command.ExecuteReaderAsync(
            cancellationToken);
        while (await reader.ReadAsync(cancellationToken))
        {
            paths.Add(reader.GetString(0));
        }

        return paths;
    }

    private async Task<IReadOnlyList<FaceRecord>> QueryFacesAsync(
        string whereClause,
        long? personParameter,
        string? pathParameter,
        CancellationToken cancellationToken)
    {
        await using var connection = await OpenConnectionAsync(cancellationToken);
        await using var command = connection.CreateCommand();
        command.CommandText =
            $"""
            SELECT id, path, x, y, w, h, confidence, embedding, person_id,
                   suggested_person_id
            FROM faces
            {whereClause}
            ORDER BY confidence DESC;
            """;
        if (personParameter is { } person)
        {
            command.Parameters.AddWithValue("$person", person);
        }

        if (pathParameter is { } pathValue)
        {
            command.Parameters.AddWithValue("$path", pathValue);
        }

        var faces = new List<FaceRecord>();
        await using var reader = await command.ExecuteReaderAsync(
            cancellationToken);
        while (await reader.ReadAsync(cancellationToken))
        {
            faces.Add(new FaceRecord(
                reader.GetInt64(0),
                reader.GetString(1),
                reader.GetDouble(2),
                reader.GetDouble(3),
                reader.GetDouble(4),
                reader.GetDouble(5),
                reader.GetDouble(6),
                BlobToEmbedding((byte[])reader.GetValue(7)),
                reader.IsDBNull(8) ? null : reader.GetInt64(8),
                reader.IsDBNull(9) ? null : reader.GetInt64(9)));
        }

        return faces;
    }

    public async Task<IReadOnlyList<PersonRecord>> GetPeopleAsync(
        CancellationToken cancellationToken = default)
    {
        await using var connection = await OpenConnectionAsync(cancellationToken);
        await using var command = connection.CreateCommand();
        command.CommandText =
            """
            SELECT people.id, people.name, COUNT(faces.id)
            FROM people
            LEFT JOIN faces ON faces.person_id = people.id
            GROUP BY people.id, people.name
            ORDER BY people.name COLLATE NOCASE;
            """;
        var people = new List<PersonRecord>();
        await using var reader = await command.ExecuteReaderAsync(
            cancellationToken);
        while (await reader.ReadAsync(cancellationToken))
        {
            people.Add(new PersonRecord(
                reader.GetInt64(0),
                reader.GetString(1),
                reader.GetInt32(2)));
        }

        return people;
    }

    /// <summary>
    /// Finds a person by name - without regard to case, so "jana" and
    /// "Jana" stay one person - or creates them.
    /// </summary>
    public async Task<long> GetOrCreatePersonAsync(
        string name,
        CancellationToken cancellationToken = default)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(name);
        var trimmed = name.Trim();
        await using var connection = await OpenConnectionAsync(cancellationToken);
        await using (var find = connection.CreateCommand())
        {
            find.CommandText =
                "SELECT id FROM people WHERE name = $name COLLATE NOCASE;";
            find.Parameters.AddWithValue("$name", trimmed);
            if (await find.ExecuteScalarAsync(cancellationToken)
                is long existing)
            {
                return existing;
            }
        }

        await using var insert = connection.CreateCommand();
        insert.CommandText =
            """
            INSERT INTO people (name, created_utc)
            VALUES ($name, $created);
            SELECT last_insert_rowid();
            """;
        insert.Parameters.AddWithValue("$name", trimmed);
        insert.Parameters.AddWithValue(
            "$created",
            DateTime.UtcNow.ToString("O"));
        return (long)(await insert.ExecuteScalarAsync(cancellationToken))!;
    }

    public async Task AssignFacesAsync(
        IReadOnlyCollection<long> faceIds,
        long? personId,
        CancellationToken cancellationToken = default)
    {
        if (faceIds.Count == 0)
        {
            return;
        }

        await using var connection = await OpenConnectionAsync(cancellationToken);
        await using var transaction = await connection.BeginTransactionAsync(
            cancellationToken);
        await using var command = connection.CreateCommand();
        command.Transaction = (SqliteTransaction)transaction;
        // Deciding on a face settles it either way, so any pending
        // suggestion is cleared alongside the assignment.
        command.CommandText =
            """
            UPDATE faces
            SET person_id = $person, suggested_person_id = NULL
            WHERE id = $id;
            """;
        command.Parameters.AddWithValue(
            "$person",
            personId is { } person ? person : DBNull.Value);
        var id = command.Parameters.Add("$id", SqliteType.Integer);
        foreach (var faceId in faceIds)
        {
            cancellationToken.ThrowIfCancellationRequested();
            id.Value = faceId;
            await command.ExecuteNonQueryAsync(cancellationToken);
        }

        await transaction.CommitAsync(cancellationToken);
    }

    /// <summary>Rejects suggestions: the faces return to the unnamed pool.</summary>
    public async Task ClearSuggestionsAsync(
        IReadOnlyCollection<long> faceIds,
        CancellationToken cancellationToken = default)
    {
        if (faceIds.Count == 0)
        {
            return;
        }

        await using var connection = await OpenConnectionAsync(cancellationToken);
        await using var command = connection.CreateCommand();
        command.CommandText =
            "UPDATE faces SET suggested_person_id = NULL WHERE id = $id;";
        var id = command.Parameters.Add("$id", SqliteType.Integer);
        foreach (var faceId in faceIds)
        {
            cancellationToken.ThrowIfCancellationRequested();
            id.Value = faceId;
            await command.ExecuteNonQueryAsync(cancellationToken);
        }
    }

    /// <summary>
    /// Queues the photograph's named face rectangles for exiftool to write
    /// as MWG regions; the payload carries the pixel dimensions and the
    /// normalized top-left rectangles with their names.
    /// </summary>
    public async Task EnqueueFaceRegionsAsync(
        string path,
        string payloadJson,
        CancellationToken cancellationToken = default)
    {
        await using var connection = await OpenConnectionAsync(cancellationToken);
        await using var transaction = await connection.BeginTransactionAsync(
            cancellationToken);
        await EnqueueOutboxAsync(
            connection,
            (SqliteTransaction)transaction,
            path,
            "regions",
            payloadJson,
            cancellationToken);
        await transaction.CommitAsync(cancellationToken);
        MetadataOutboxChanged?.Invoke();
    }

    public async Task RenamePersonAsync(
        long personId,
        string name,
        CancellationToken cancellationToken = default)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(name);
        await using var connection = await OpenConnectionAsync(cancellationToken);
        await using var command = connection.CreateCommand();
        command.CommandText =
            "UPDATE people SET name = $name WHERE id = $id;";
        command.Parameters.AddWithValue("$name", name.Trim());
        command.Parameters.AddWithValue("$id", personId);
        await command.ExecuteNonQueryAsync(cancellationToken);
    }

    /// <summary>
    /// Removes a person; their faces stay in the catalogue and return to the
    /// unnamed pool. Keywords already written into files are left alone.
    /// </summary>
    public async Task DeletePersonAsync(
        long personId,
        CancellationToken cancellationToken = default)
    {
        await using var connection = await OpenConnectionAsync(cancellationToken);
        await using var command = connection.CreateCommand();
        command.CommandText =
            """
            UPDATE faces SET person_id = NULL WHERE person_id = $id;
            UPDATE faces SET suggested_person_id = NULL
            WHERE suggested_person_id = $id;
            DELETE FROM people WHERE id = $id;
            """;
        command.Parameters.AddWithValue("$id", personId);
        await command.ExecuteNonQueryAsync(cancellationToken);
    }

    internal static byte[] EmbeddingToBlob(float[] embedding)
    {
        var blob = new byte[embedding.Length * sizeof(float)];
        Buffer.BlockCopy(embedding, 0, blob, 0, blob.Length);
        return blob;
    }

    internal static float[] BlobToEmbedding(byte[] blob)
    {
        var embedding = new float[blob.Length / sizeof(float)];
        Buffer.BlockCopy(blob, 0, embedding, 0, embedding.Length * sizeof(float));
        return embedding;
    }
}
