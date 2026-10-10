package com.flashmind.beep

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import java.util.Locale

class ProductTest {
    @Test fun readsEveryField() {
        val p = parseProduct("""{"title":"Stick vacuum","url":"https://shop.example/item","price":"€249","was":"€329","store":"Shop","image":"https://shop.example/img.jpg","rating":4.6,"reviews":1204,"note":"Quietest"}""")!!
        assertEquals("Stick vacuum", p.title)
        assertEquals("€249", p.price)
        assertEquals("€329", p.was)
        assertEquals("Shop", p.store)
        assertEquals("https://shop.example/img.jpg", p.image)
        assertEquals(4.6, p.rating!!, 0.0)
        assertEquals(1204, p.reviews)
        assertEquals("Quietest", p.note)
    }

    @Test fun storeDefaultsToHost() {
        val p = parseProduct("""{"title":"A","url":"https://www.shop.example/a"}""")!!
        assertEquals("shop.example", p.store)
        assertNull(p.price)
        assertNull(p.details)
    }

    @Test fun titleAndUrlAreRequired() {
        assertNull(parseProduct("""{"url":"https://shop.example/a"}"""))
        assertNull(parseProduct("""{"title":"A"}"""))
        assertNull(parseProduct("""{"title":"A","url":"javascript:alert(1)"}"""))
        assertNull(parseProduct("""{"title":"A","url":"https://shop.example/a""""))
        assertNull(parseProduct("not json"))
    }

    @Test fun detailsLine() {
        val p = parseProduct("""{"title":"A","url":"https://shop.example/a","rating":4,"note":"Light"}""")!!
        assertEquals("4 ★ · Light", p.details)
    }

    @Test fun shownProductsFormatPrices() {
        Locale.setDefault(Locale.US)
        val shown = shownProducts(JSONObject("""{"action":"show_products","products":[
            {"title":"Kettle","url":"https://shop.example/k","store":"shop.example","price":249,"was":329,"currency":"EUR"},
            {"title":"Mug","url":"https://shop.example/m","store":"shop.example","price":19.5,"currency":"USD"},
            {"title":"","url":"https://shop.example/x","store":"shop.example"},
            "not an object",
            {"title":"Lamp","url":"https://shop.example/l","store":"shop.example","price":12,"currency":"nope"}]}"""))
        assertEquals(listOf("Kettle", "Mug", "Lamp"), shown.map { it.title })
        assertEquals("€249", shown[0].price)
        assertEquals("€329", shown[0].was)
        assertEquals("$19.50", shown[1].price)
        assertNull(shown[2].price)
    }
}
