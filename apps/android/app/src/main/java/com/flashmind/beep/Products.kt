package com.flashmind.beep

import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.snapping.rememberSnapFlingBehavior
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.OpenInNew
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.json.JSONObject
import java.net.URI
import java.text.NumberFormat
import java.util.Currency

/** A product Beep put in a reply as a ```product block. */
data class Product(
    val title: String,
    val url: String,
    val price: String? = null,
    val was: String? = null,
    val store: String,
    val image: String? = null,
    val rating: Double? = null,
    val reviews: Int? = null,
    val note: String? = null,
) {
    /** "4.6 ★ (1,204) · Quietest of the three", or whichever parts there are. */
    val details: String? get() {
        val stars = rating?.let { r ->
            val count = reviews?.let { " (${NumberFormat.getIntegerInstance().format(it)})" } ?: ""
            "${if (r % 1.0 == 0.0) r.toInt().toString() else r.toString()} ★$count"
        }
        return listOfNotNull(stars, note).joinToString(" · ").ifEmpty { null }
    }
}

/**
 * Reads one product block. Title and an http(s) url are required; the store defaults to
 * the url's host. Returns null for anything else, so the block shows as plain code.
 */
internal fun parseProduct(json: String): Product? = runCatching {
    val o = JSONObject(json.trim())
    fun text(key: String) = o.optString(key, "").trim().takeIf { o.has(key) && !o.isNull(key) && it.isNotEmpty() }
    val title = text("title") ?: return null
    val url = text("url")?.takeIf { it.startsWith("https://") || it.startsWith("http://") } ?: return null
    val host = runCatching { URI(url).host }.getOrNull()?.removePrefix("www.") ?: return null
    Product(
        title = title,
        url = url,
        price = text("price"),
        was = text("was"),
        store = text("store") ?: host,
        image = text("image")?.takeIf { it.startsWith("https://") },
        rating = o.optDouble("rating").takeIf { !it.isNaN() && it > 0 },
        reviews = o.optInt("reviews", -1).takeIf { it >= 0 },
        note = text("note"),
    )
}.getOrNull()

/**
 * The products a show_products result lists. The server has checked them; an entry that
 * still does not read is skipped.
 */
internal fun shownProducts(json: JSONObject): List<Product> {
    val list = json.optJSONArray("products") ?: return emptyList()
    return (0 until list.length()).mapNotNull { i ->
        val o = list.optJSONObject(i) ?: return@mapNotNull null
        val currency = o.optString("currency", "")
        fun amount(key: String) = o.optDouble(key).takeIf { !it.isNaN() && it > 0 }?.let { price(it, currency) }
        val product = parseProduct(o.toString()) ?: return@mapNotNull null
        val price = amount("price")
        product.copy(price = price, was = if (price != null) amount("was") else null)
    }
}

/** [amount] in [currency] the way this phone writes money, without cents when there are none. */
internal fun price(amount: Double, currency: String): String? = runCatching {
    val format = NumberFormat.getCurrencyInstance().apply { this.currency = Currency.getInstance(currency) }
    if (amount % 1.0 == 0.0) format.maximumFractionDigits = 0
    format.format(amount)
}.getOrNull()

/** What a tap on a card does: the built-in browser when the app gives one, else the phone's browser. */
internal val LocalOpenLink = compositionLocalOf<((String) -> Unit)?> { null }

/**
 * Products side by side in a row that scrolls and snaps; a lone product takes the full width.
 * [loading] adds a placeholder card for a block still streaming in.
 */
@Composable
internal fun ProductRow(products: List<Product>, loading: Boolean) {
    if (products.size == 1 && !loading) { ProductCard(products[0], Modifier.fillMaxWidth()); return }
    val state = rememberLazyListState()
    LazyRow(state = state, flingBehavior = rememberSnapFlingBehavior(state),
        horizontalArrangement = Arrangement.spacedBy(10.dp), modifier = Modifier.fillMaxWidth()) {
        items(products, key = { it.url + it.title }) { ProductCard(it, Modifier.width(208.dp).padding(bottom = 4.dp)) }
        if (loading) item { LoadingCard(Modifier.width(208.dp).padding(bottom = 4.dp)) }
    }
}

@Composable
private fun ProductCard(product: Product, modifier: Modifier) {
    val context = LocalContext.current
    val open = LocalOpenLink.current
    val shape = RoundedCornerShape(16.dp)
    Box(modifier) {
        Column(Modifier.fillMaxWidth().beepFrame(shape, Beep.colors.surface)
            .clickable(onClickLabel = "Open ${product.store}") { open?.invoke(product.url) ?: ExternalBrowser.open(context, product.url) }) {
            ProductImage(product.image)
            Column(Modifier.padding(start = 12.dp, end = 12.dp, top = 10.dp, bottom = 12.dp), verticalArrangement = Arrangement.spacedBy(5.dp)) {
                Text(product.store, color = Beep.colors.muted, fontSize = 11.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(product.title, color = Beep.colors.ink, fontSize = 13.5.sp, lineHeight = 18.sp, fontWeight = FontWeight.Medium,
                    maxLines = 2, minLines = 2, overflow = TextOverflow.Ellipsis)
                if (product.price != null) Row(verticalAlignment = Alignment.Bottom, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    Text(product.price, color = Beep.colors.ink, fontSize = 17.sp, fontWeight = FontWeight.SemiBold)
                    product.was?.let { Text(it, color = Beep.colors.muted, fontSize = 12.sp, textDecoration = TextDecoration.LineThrough) }
                } else Text("See price on the site", color = Beep.colors.muted, fontSize = 12.5.sp)
                product.details?.let { Text(it, color = Beep.colors.muted, fontSize = 12.sp, maxLines = 1, overflow = TextOverflow.Ellipsis) }
            }
        }
        Box(Modifier.align(Alignment.TopEnd).padding(8.dp).size(30.dp).clip(CircleShape)
            .background(Beep.colors.surface).border(1.5.dp, Beep.colors.ledge, CircleShape)
            .clickable(onClickLabel = "Open in your browser") { ExternalBrowser.open(context, product.url) },
            contentAlignment = Alignment.Center) {
            Icon(Icons.AutoMirrored.Outlined.OpenInNew, "Open in your browser", Modifier.size(16.dp), tint = Beep.colors.ink)
        }
    }
}

@Composable
private fun ProductImage(url: String?) {
    val bitmap by produceState<ImageBitmap?>(null, url) {
        value = url?.let { ReplyImages.load(it) }?.asImageBitmap()
    }
    Box(Modifier.fillMaxWidth().height(116.dp).background(Beep.colors.code)) {
        bitmap?.let { Image(it, null, Modifier.fillMaxWidth().height(116.dp), contentScale = ContentScale.Crop) }
    }
}

/** A card shaped like the others while its block is still coming in. */
@Composable
private fun LoadingCard(modifier: Modifier) {
    Column(modifier.beepFrame(RoundedCornerShape(16.dp), Beep.colors.surface)) {
        Box(Modifier.fillMaxWidth().height(116.dp).background(Beep.colors.code))
        Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            listOf(0.4f, 0.9f, 0.6f, 0.3f).forEach { w ->
                Box(Modifier.fillMaxWidth(w).heightIn(min = 10.dp).clip(RoundedCornerShape(5.dp)).background(Beep.colors.code))
            }
        }
    }
}
