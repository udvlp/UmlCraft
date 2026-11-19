// Compile stub for CI: only the part of Astah's API that AstaExport.java uses, declared
// as the API declares it (see tools/asta-export/README.md).
package com.change_vision.jude.api.inf.model;

public interface IAssociationClass extends com.change_vision.jude.api.inf.model.IClass, com.change_vision.jude.api.inf.model.IAssociation {
    public abstract void setAbstract(boolean p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract void setLeaf(boolean p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract void setActive(boolean p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract com.change_vision.jude.api.inf.model.IElement getOwner();
}
