package io.ytmlisten.player

import org.json.JSONArray
import org.json.JSONObject

/** Converts the official read-only playlist API to the shared catalog. No credentials in results. */
object YouTubeLibrary {
    const val PREFIX = "ytapi_"
    private val playlistId = Regex("[A-Za-z0-9_-]{1,200}")
    private val videoId = Regex("[A-Za-z0-9_-]{11}")

    fun query(request: JSONObject, fetch: (String, Map<String, String>) -> JSONObject): JSONObject {
        val kind = request.optString("kind")
        val browse = request.optString("browse_id")
        val continuation = request.optString("continuation")
        require(continuation.length <= 4096) { "Invalid library page" }
        val library = kind == "library"
        require(library || (kind == "browse" && browse.startsWith(PREFIX))) { "Invalid library request" }
        val id = if (library) "" else browse.removePrefix(PREFIX)
        require(library || playlistId.matches(id)) { "Invalid playlist" }
        val params = mutableMapOf("part" to "snippet,contentDetails", "maxResults" to "50")
        if (library) params["mine"] = "true" else params["playlistId"] = id
        if (continuation.isNotEmpty()) params["pageToken"] = continuation
        val response = fetch(if (library) "playlists" else "playlistItems", params)
        val source = response.optJSONArray("items") ?: JSONArray()
        val items = JSONArray()
        for (index in 0 until minOf(source.length(), 50)) {
            val item = source.optJSONObject(index) ?: continue
            val snippet = item.optJSONObject("snippet") ?: continue
            val title = snippet.optString("title").take(512)
            val key = if (library) item.optString("id") else item.optJSONObject("contentDetails")?.optString("videoId").orEmpty()
            if (title.isBlank() || if (library) !playlistId.matches(key) else !videoId.matches(key)) continue
            val thumbnails = snippet.optJSONObject("thumbnails")
            val image = listOf("high", "medium", "default").firstNotNullOfOrNull {
                thumbnails?.optJSONObject(it)?.optString("url")?.takeIf { url -> url.startsWith("https://") && url.length <= 2048 }
            }.orEmpty()
            val artist = snippet.optString("videoOwnerChannelTitle").ifBlank { snippet.optString("channelTitle") }.take(512)
            items.put(JSONObject().put("key", if (library) PREFIX + key else "${item.optString("id").take(200)}:$key")
                .put("title", title).put("artist", artist).put("subtitle", artist)
                .put("kind", if (library) "playlist" else "song").put("artwork", image)
                .put("browse_id", if (library) PREFIX + key else "").put("video_id", if (library) "" else key))
        }
        return JSONObject().put("title", if (library) "Your YouTube playlists" else "Playlist")
            .put("items", items).put("signed_in", true)
            .put("message", "Google library connected. Playback uses a separate session; Music Premium sign-in is not enabled.")
            .put("continuation", response.optString("nextPageToken").take(4096)).put("playlist_id", id)
    }
}
