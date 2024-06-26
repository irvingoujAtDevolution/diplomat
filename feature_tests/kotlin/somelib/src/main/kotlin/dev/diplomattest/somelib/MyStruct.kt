package dev.diplomattest.somelib

import com.sun.jna.Library
import com.sun.jna.Native
import com.sun.jna.Pointer
import com.sun.jna.Structure

internal interface MyStructLib: Library {
    fun MyStruct_new(): MyStructNative
    fun MyStruct_into_a(nativeStruct: MyStructNative): UByte
}

class MyStructNative: Structure(), Structure.ByValue {
    @JvmField
    var a: Byte = 0;
    @JvmField
    var b: Byte = 0;
    @JvmField
    var c: Byte = 0;
    @JvmField
    var d: Long = 0;
    @JvmField
    var e: Int = 0;
    @JvmField
    var f: Int = 0;
    @JvmField
    var g: Int = MyEnum.default().toNative();
  
    // Define the fields of the struct
    override fun getFieldOrder(): List<String> {
        return listOf("a", "b", "c", "d", "e", "f", "g")
    }
}

class MyStruct internal constructor (
    internal val nativeStruct: MyStructNative) {
    val a: UByte = nativeStruct.a.toUByte()
    val b: Boolean = nativeStruct.b > 0
    val c: UByte = nativeStruct.c.toUByte()
    val d: ULong = nativeStruct.d.toULong()
    val e: Int = nativeStruct.e
    val f: Int = nativeStruct.f
    val g: MyEnum = MyEnum.fromNative(nativeStruct.g)

    companion object {
        internal val libClass: Class<MyStructLib> = MyStructLib::class.java
        internal val lib: MyStructLib = Native.load("somelib", libClass)
        val NATIVESIZE: Long = Native.getNativeSize(MyStructNative::class.java).toLong()
        fun new_(): MyStruct {
            
            val returnVal = lib.MyStruct_new();
        
            val returnStruct = MyStruct(returnVal)
            return returnStruct 
        
        }
    }
    fun intoA(): UByte {
        
        val returnVal = lib.MyStruct_into_a(nativeStruct);
        return returnVal
    }

}
