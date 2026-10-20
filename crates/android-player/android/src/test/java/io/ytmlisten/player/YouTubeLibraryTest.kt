package io.ytmlisten.player

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class YouTubeLibraryTest {
    @Test fun ownPlaylistsHaveNativeBrowseIdsAndPageTokens() {
        val request = JSONObject("""{"kind":"library","continuation":"page two"}""")
        val result = YouTubeLibrary.query(request) { endpoint, params ->
            assertEquals("playlists", endpoint)
            assertEquals("true", params["mine"])
            assertEquals("page two", params["pageToken"])
            JSONObject("""{"items":[{"id":"PL_one","snippet":{"title":"Private playlist","channelTitle":"My channel","thumbnails":{"high":{"url":"https://i.ytimg.com/example.jpg"}}}}],"nextPageToken":"three"}""")
        }
        assertTrue(result.getBoolean("signed_in"))
        assertEquals("three", result.getString("continuation"))
        val item = result.getJSONArray("items").getJSONObject(0)
        assertEquals("ytapi_PL_one", item.getString("browse_id"))
        assertEquals("", item.getString("video_id"))
        assertEquals("playlist", item.getString("kind"))
        assertFalse(result.toString().contains("Bearer"))
    }

    @Test fun playlistEntriesPreserveDuplicatesAndSkipUnavailableVideos() {
        val result = YouTubeLibrary.query(JSONObject("""{"kind":"browse","browse_id":"ytapi_PL_one"}""")) { endpoint, params ->
            assertEquals("playlistItems", endpoint)
            assertEquals("PL_one", params["playlistId"])
            JSONObject("""{"items":[{"id":"a","contentDetails":{"videoId":"dQw4w9WgXcQ"},"snippet":{"title":"First","videoOwnerChannelTitle":"Artist"}},{"id":"b","contentDetails":{"videoId":"dQw4w9WgXcQ"},"snippet":{"title":"Again"}},{"snippet":{"title":"Private video"}}]}""")
        }
        val items = result.getJSONArray("items")
        assertEquals(2, items.length())
        assertNotEquals(items.getJSONObject(0).getString("key"), items.getJSONObject(1).getString("key"))
        assertEquals("Artist", items.getJSONObject(0).getString("artist"))
        assertEquals("PL_one", result.getString("playlist_id"))
    }

    @Test fun foreignBrowseIdsCannotBecomeApiRequests() {
        for (id in listOf("PL_other", "ytapi_../../userinfo", "ytapi_")) {
            try {
                YouTubeLibrary.query(JSONObject().put("kind", "browse").put("browse_id", id)) { _, _ -> fail("Must not request network"); JSONObject() }
                fail("Must reject $id")
            } catch (_: IllegalArgumentException) { }
        }
    }
}
