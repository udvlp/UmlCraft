// Compile stub for CI: only the part of Astah's API that AstaExport.java uses, declared
// as the API declares it (see tools/asta-export/README.md).
package com.change_vision.jude.api.inf.model;

public interface IElement {
    public abstract java.lang.String getId();
    public abstract boolean hasStereotype(java.lang.String p0);
    public abstract void addStereotype(java.lang.String p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
}
