// Compile stub for CI: only the part of Astah's API that AstaExport.java uses, declared
// as the API declares it (see tools/asta-export/README.md).
package com.change_vision.jude.api.inf.model;

public interface IERSubtypeRelationship extends com.change_vision.jude.api.inf.model.INamedElement {
    public abstract com.change_vision.jude.api.inf.model.IEREntity getParent();
    public abstract com.change_vision.jude.api.inf.model.IEREntity getChild();
    public abstract void setDiscriminatorAttribute(com.change_vision.jude.api.inf.model.IERAttribute p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract void setConclusive(boolean p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
}
