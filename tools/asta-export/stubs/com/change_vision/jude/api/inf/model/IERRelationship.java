// Compile stub for CI: only the part of Astah's API that AstaExport.java uses, declared
// as the API declares it (see tools/asta-export/README.md).
package com.change_vision.jude.api.inf.model;

public interface IERRelationship extends com.change_vision.jude.api.inf.model.INamedElement {
    public abstract com.change_vision.jude.api.inf.model.IEREntity getParent();
    public abstract com.change_vision.jude.api.inf.model.IEREntity getChild();
    public abstract void setVerbPhraseParent(java.lang.String p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract void setVerbPhraseChild(java.lang.String p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract void setParentRequired(boolean p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract void setCardinality(java.lang.String p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
}
